// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 查询执行线程：DataFusion 计划在这里执行，不占用处理 HTTP、数据库连接与后台任务的 runtime。
//!
//! 执行计划是 CPU 活。一个任务把读进来的整个行组（约 128 批）解码、过滤、聚合完才让出线程，
//! 合并后的大段一次就要一两秒；几条查询同时这样做，主 runtime 的工作线程全被占住，期间谁也
//! 推进不了：数据库连接拿到了还不回去，别的请求等满获取超时也拿不到连接。计划改在这里的一组
//! 线程上执行，两边的线程由操作系统分配 CPU，主 runtime 的任务醒来就能运行。
//!
//! 规划仍在调用方的 runtime 上做：读目录、剪枝要访问数据库，放到执行线程上，执行线程忙时
//! 就会一直占着连接。

use std::{
    future::Future,
    sync::{Arc, OnceLock},
};

use arrow::array::RecordBatch;
use datafusion::{
    error::{DataFusionError, Result},
    physical_plan,
    prelude::DataFrame,
};
use signals::trace_context::{current_trace_context, with_current_trace_context};
use tokio::runtime::{Builder, Handle, Runtime};
use tokio_util::task::AbortOnDropHandle;
use tracing::Instrument;

/// 执行线程的名字，日志与 CPU 剖析里靠它认出查询执行。
const THREAD_NAME: &str = "query-executor";

/// 在调用方的 runtime 上规划 `dataframe`，在执行线程上执行，收集全部结果。
pub async fn collect(dataframe: DataFrame) -> Result<Vec<RecordBatch>> {
    let task_ctx = Arc::new(dataframe.task_ctx());
    let plan = dataframe.create_physical_plan().await?;

    execute(physical_plan::collect(plan, task_ctx)).await
}

/// 在执行线程上运行 `work`，带着调用方的 span 与 trace 上下文。调用方被丢弃（请求断开、超时）
/// 时，执行随之取消。
async fn execute<T>(work: impl Future<Output = Result<T>> + Send + 'static) -> Result<T>
where
    T: Send + 'static,
{
    let work = work.in_current_span();
    let context = current_trace_context();

    let task = AbortOnDropHandle::new(handle().spawn(async move {
        match context {
            Some(context) => with_current_trace_context(context, work).await,
            None => work.await,
        }
    }));

    match task.await {
        Ok(result) => result,
        Err(error) if error.is_panic() => std::panic::resume_unwind(error.into_panic()),
        Err(error) => Err(DataFusionError::External(Box::new(error))),
    }
}

/// 执行线程所在的 runtime：线程数与 CPU 数相同（容器里按配额算），第一次执行时启动，之后
/// 一直存在。
fn handle() -> &'static Handle {
    static RUNTIME: OnceLock<Runtime> = OnceLock::new();

    RUNTIME
        .get_or_init(|| {
            Builder::new_multi_thread()
                .worker_threads(std::thread::available_parallelism().map_or(1, usize::from))
                .thread_name(THREAD_NAME)
                .enable_all()
                .build()
                .expect("the query execution threads start")
        })
        .handle()
}

#[cfg(test)]
mod tests {
    use std::{panic::AssertUnwindSafe, time::Duration};

    use arrow::array::{Array, Int64Array};
    use datafusion::prelude::SessionContext;
    use futures::FutureExt;
    use tokio::sync::oneshot;

    use super::*;

    /// 被丢弃时报信。
    struct Dropped(Option<oneshot::Sender<()>>);

    impl Drop for Dropped {
        fn drop(&mut self) {
            if let Some(sender) = self.0.take() {
                let _ = sender.send(());
            }
        }
    }

    #[tokio::test]
    async fn a_plan_answers_as_datafusion_does() {
        let context = SessionContext::new();
        let frame = context.sql("SELECT 1 + 1 AS two").await.unwrap();
        let batches = collect(frame).await.unwrap();

        let column = batches[0]
            .column(0)
            .as_any()
            .downcast_ref::<Int64Array>()
            .unwrap();

        assert_eq!((column.len(), column.value(0)), (1, 2));
    }

    #[tokio::test]
    async fn a_failing_plan_hands_back_its_own_error() {
        let context = SessionContext::new();
        let frame = context.sql("SELECT CAST('x' AS INT) AS n").await.unwrap();
        let error = collect(frame).await.unwrap_err();

        assert!(!matches!(error, DataFusionError::External(_)), "{error}");
    }

    #[tokio::test]
    async fn the_work_runs_on_the_execution_threads() {
        let name = execute(async { Ok(std::thread::current().name().map(str::to_string)) })
            .await
            .unwrap();

        assert_eq!(name.as_deref(), Some(THREAD_NAME));
    }

    #[tokio::test]
    async fn a_caller_that_goes_away_cancels_the_work() {
        let (report_drop, dropped) = oneshot::channel();
        let (report_start, started) = oneshot::channel();

        let mut work = Box::pin(execute(async move {
            let _dropped = Dropped(Some(report_drop));
            let _ = report_start.send(());
            std::future::pending::<Result<()>>().await
        }));

        tokio::select! {
            _ = &mut work => panic!("the work never finishes"),
            result = started => result.unwrap(),
        }

        drop(work);

        tokio::time::timeout(Duration::from_secs(5), dropped)
            .await
            .expect("the work is cancelled")
            .unwrap();
    }

    #[tokio::test]
    async fn a_panic_in_the_work_reaches_the_caller() {
        let work = execute(async {
            if std::thread::current().name() == Some(THREAD_NAME) {
                panic!("the work panics");
            }

            Ok(())
        });

        assert!(AssertUnwindSafe(work).catch_unwind().await.is_err());
    }
}
