// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Running the statements of the RUM pages and of the tools that read what the pages read.

use std::time::Duration;

use crate::{
    app::{iam::IamContext, query::QueryService},
    domain::{
        query::{QueryLanguage, QueryRequest, QueryResult, StreamHint},
        rum::event::RumStream,
        stream::{StreamRepository, StreamType},
    },
    infra::rum::query::Unavailable,
    shared::{
        Error, Result,
        ids::Id,
        time::{TimeRange, TimestampMicros},
    },
};

/// A count gives up before the web client's own request limit of 30 s, so the page can fall back
/// to the rows that are loaded instead of failing.
const TIME_BUDGET: Duration = Duration::from_secs(20);

/// Runs `work` inside the time budget. Why there is no answer is told rather than failed with: a
/// busy role and a statement that took too long both leave the reader something to say.
pub async fn within_budget<T>(
    work: impl Future<Output = Result<T>>,
) -> Result<std::result::Result<T, Unavailable>> {
    match tokio::time::timeout(TIME_BUDGET, work).await {
        Ok(Ok(value)) => Ok(Ok(value)),
        Ok(Err(Error::ResourceExhausted(_))) => Ok(Err(Unavailable::Busy)),
        Ok(Err(error)) => Err(error),
        Err(_) => Ok(Err(Unavailable::TimeBudget)),
    }
}

/// A range of time in microseconds, `[from, to)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Span {
    pub from: i64,
    pub to: i64,
}

/// What a counting statement came to.
pub enum Counted {
    /// Its rows; none at all when nothing was ever written to the stream.
    Rows(Option<QueryResult>),
    /// Why there is no count.
    Unavailable(Unavailable),
}

/// Runs statements for one reader. The query service masks what the reader may not see.
pub struct Reader<'a> {
    query: &'a QueryService,
    streams: &'a dyn StreamRepository,
    iam: &'a IamContext,
}

impl<'a> Reader<'a> {
    pub fn new(
        query: &'a QueryService,
        streams: &'a dyn StreamRepository,
        iam: &'a IamContext,
    ) -> Self {
        Self {
            query,
            streams,
            iam,
        }
    }

    /// The organization the reader reads for.
    pub fn org_id(&self) -> &Id {
        &self.iam.org_id
    }

    /// The answer of a statement over one stream, or `None` when nothing was ever written to it.
    pub async fn run(
        &self,
        stream: RumStream,
        span: Span,
        statement: String,
        limit: Option<usize>,
    ) -> Result<Option<QueryResult>> {
        if !self.present(stream).await? {
            return Ok(None);
        }

        self.query
            .run(self.request(stream, span, statement, limit))
            .await
            .map(Some)
    }

    /// The same, admitted by the reader's role and listed among the running queries: for the
    /// statements that read many rows to count them.
    pub async fn run_tracked(
        &self,
        stream: RumStream,
        span: Span,
        statement: String,
    ) -> Result<Option<QueryResult>> {
        if !self.present(stream).await? {
            return Ok(None);
        }

        self.query
            .run_tracked(
                self.request(stream, span, statement, None),
                self.iam.user_id.clone(),
                self.iam.organization_role_key(),
            )
            .await
            .map(Some)
    }

    /// A statement that counts, inside the time budget: a page that is told why there is no count
    /// can fall back to the rows it has loaded.
    pub async fn count(&self, stream: RumStream, span: Span, statement: String) -> Result<Counted> {
        match within_budget(self.run_tracked(stream, span, statement)).await? {
            Ok(result) => Ok(Counted::Rows(result)),
            Err(reason) => Ok(Counted::Unavailable(reason)),
        }
    }

    /// Whether anything was ever written to the stream.
    pub async fn has(&self, stream: RumStream) -> Result<bool> {
        self.present(stream).await
    }

    async fn present(&self, stream: RumStream) -> Result<bool> {
        match self
            .streams
            .get(&self.iam.org_id, stream.name(), StreamType::LOGS)
            .await
        {
            Ok(_) => Ok(true),
            Err(Error::NotFound(_)) => Ok(false),
            Err(error) => Err(error),
        }
    }

    fn request(
        &self,
        stream: RumStream,
        span: Span,
        statement: String,
        limit: Option<usize>,
    ) -> QueryRequest {
        QueryRequest {
            org_id: self.iam.org_id.clone(),
            language: QueryLanguage::Sql,
            statement,
            time_range: TimeRange::new(TimestampMicros(span.from), TimestampMicros(span.to)),
            stream: Some(StreamHint {
                name: stream.name().to_string(),
                stream_type: StreamType::LOGS,
            }),
            limit,
            step: None,
            federation_clusters: Vec::new(),
        }
    }
}
