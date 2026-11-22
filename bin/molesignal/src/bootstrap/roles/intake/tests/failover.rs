// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! Losing a node, its disk, its lease or the object store, and draining it: every acknowledged
//! row is published exactly once, and nothing unacknowledged is reported as written.

use domain::cluster::PeerRole;

use super::*;
use crate::{app::cluster::WorkOwnership, bootstrap::roles::intake::takeover::TakeoverRound};

fn solo() -> Arc<WorkOwnership> {
    WorkOwnership::solo(PeerRole::Intake)
}

/// The node is gone, memory and disk: all that is left of it is in object storage and Postgres.
fn lose(fixture: &WorkerFixture, worker: Arc<IntakeWorker>) -> WriterEpoch {
    let epoch = worker.wal().lease().epoch();
    drop(worker);
    fixture.registry.expire(epoch);
    epoch
}

#[tokio::test]
async fn rows_acknowledged_before_the_node_and_its_disk_are_lost_are_published_by_another() {
    let fixture = fixture(IntakeSettings::default()).await;
    let a = fixture.worker().clone();

    for offset in 0..3 {
        a.write(batch(&fixture.stream, 1_000_000 + offset * 10, 4))
            .await
            .unwrap();
    }

    assert_eq!(
        objects(&fixture.store, "wal/").await,
        3,
        "acknowledged = in object storage"
    );
    let dead = lose(&fixture, a);

    // The same node id comes back without its disk (or another node): it takes over.
    let b = fixture.node().await;
    let round = b.take_over_lapsed(&solo()).await.unwrap();

    assert_eq!(
        round,
        TakeoverRound {
            fenced: 1,
            streams: 1,
            rows: 12
        }
    );
    assert_eq!(fixture.catalog.committed_rows(), vec![12]);
    assert_eq!(objects(&fixture.store, "wal/").await, 0);
    assert!(
        fixture
            .registry
            .streams()
            .iter()
            .all(|s| s.writer_epoch != dead)
    );
    assert_eq!(
        fixture.registry.writer_count(),
        1,
        "the dead writer is forgotten"
    );

    let again = b.take_over_lapsed(&solo()).await.unwrap();
    assert_eq!(again, TakeoverRound::default());
    assert_eq!(
        fixture.catalog.committed_rows(),
        vec![12],
        "nothing published twice"
    );
}

#[tokio::test]
async fn a_takeover_publishes_only_what_the_writer_had_not_flushed() {
    let fixture = fixture(IntakeSettings::default()).await;
    let a = fixture.worker().clone();
    let key = raw_dataset_key(&fixture).await;

    a.write(batch(&fixture.stream, 1_000_000, 2)).await.unwrap();
    a.flush_one(&key).await.unwrap();
    assert_eq!(objects(&fixture.store, "wal/").await, 0);
    a.write(batch(&fixture.stream, 2_000_000, 3)).await.unwrap();
    a.write(batch(&fixture.stream, 3_000_000, 4)).await.unwrap();
    lose(&fixture, a);

    let b = fixture.node().await;
    assert_eq!(b.take_over_lapsed(&solo()).await.unwrap().rows, 7);
    assert_eq!(fixture.catalog.committed_rows(), vec![2, 7]);
}

#[tokio::test]
async fn the_old_writer_cannot_publish_once_its_wal_is_taken_over() {
    let fixture = fixture(IntakeSettings::default()).await;
    let a = fixture.worker().clone();
    let key = raw_dataset_key(&fixture).await;
    a.write(batch(&fixture.stream, 1_000_000, 5)).await.unwrap();

    // A long pause: the lease lapses while the process lives on.
    fixture.registry.expire(a.wal().lease().epoch());
    let b = fixture.node().await;
    assert_eq!(b.take_over_lapsed(&solo()).await.unwrap().rows, 5);

    let refused = a.flush_one(&key).await.unwrap_err();
    assert!(is_stale_writer_epoch(&refused), "{refused}");
    assert_eq!(fixture.catalog.committed_rows(), vec![5], "published once");
    assert_eq!(
        fixture.buffer.reserved_bytes(),
        0,
        "the old rows left memory"
    );
}

#[tokio::test]
async fn a_takeover_failing_half_way_resumes_from_the_checkpoint() {
    let mut fixture = fixture(IntakeSettings::default()).await;
    let a = fixture.worker().clone();

    for offset in 0..3 {
        a.write(batch(&fixture.stream, 1_000_000 + offset * 10, 2))
            .await
            .unwrap();
    }

    lose(&fixture, a);

    // One segment per object, and the second commit fails.
    fixture.takeover_byte_cap = 1;
    let b = fixture.node().await;
    fixture.catalog.fail_commit_after(1);

    let failed = b.take_over_lapsed(&solo()).await.unwrap();
    assert_eq!((failed.streams, failed.rows), (0, 0));
    assert_eq!(fixture.catalog.committed_rows(), vec![2]);
    assert_eq!(
        objects(&fixture.store, "wal/").await,
        3,
        "objects stay until it finishes"
    );

    let resumed = b.take_over_lapsed(&solo()).await.unwrap();
    assert_eq!((resumed.streams, resumed.rows), (1, 4));
    assert_eq!(fixture.catalog.committed_rows(), vec![2, 2, 2]);
    assert_eq!(objects(&fixture.store, "wal/").await, 0);
}

#[tokio::test]
async fn a_takeover_node_that_dies_half_way_is_taken_over_in_turn() {
    let mut fixture = fixture(IntakeSettings::default()).await;
    let a = fixture.worker().clone();

    for offset in 0..3 {
        a.write(batch(&fixture.stream, 1_000_000 + offset * 10, 2))
            .await
            .unwrap();
    }

    lose(&fixture, a);

    fixture.takeover_byte_cap = 1;
    let b = fixture.node().await;
    fixture.catalog.fail_commit_after(1);
    b.take_over_lapsed(&solo()).await.unwrap();
    assert_eq!(fixture.catalog.committed_rows(), vec![2]);
    fixture.registry.expire(b.wal().lease().epoch());

    fixture.takeover_byte_cap = usize::MAX;
    let c = fixture.node().await;
    let round = c.take_over_lapsed(&solo()).await.unwrap();
    assert_eq!((round.fenced, round.streams, round.rows), (1, 1, 4));
    assert_eq!(fixture.catalog.committed_rows(), vec![2, 4]);

    // b wakes up: its claim has moved on.
    assert_eq!(b.take_over_lapsed(&solo()).await.unwrap().streams, 0);
    assert_eq!(fixture.catalog.committed_rows(), vec![2, 4]);
}

#[tokio::test]
async fn the_wal_of_a_deleted_stream_is_dropped_by_the_takeover() {
    let fixture = fixture(IntakeSettings::default()).await;
    let a = fixture.worker().clone();
    a.write(batch(&fixture.stream, 1_000_000, 2)).await.unwrap();
    lose(&fixture, a);
    fixture.stream_gone.store(true, Ordering::SeqCst);

    let b = fixture.node().await;
    let round = b.take_over_lapsed(&solo()).await.unwrap();

    assert_eq!((round.streams, round.rows), (1, 0));
    assert!(fixture.catalog.committed_rows().is_empty());
    assert_eq!(objects(&fixture.store, "wal/").await, 0);
    assert!(fixture.registry.streams().is_empty());
}

#[tokio::test]
async fn a_failed_wal_write_is_refused_and_a_retried_one_goes_through() {
    let fixture = fixture(IntakeSettings::default()).await;
    let key = raw_dataset_key(&fixture).await;

    // Every attempt fails: refused, nothing kept in memory.
    fixture.store.fail_wal_puts(2);
    let refused = fixture
        .worker()
        .write(batch(&fixture.stream, 1_000_000, 3))
        .await
        .unwrap_err();
    assert!(matches!(refused, Error::Unavailable(_)), "{refused}");
    assert_eq!(fixture.buffer.reserved_bytes(), 0);
    assert!(
        fixture
            .buffer
            .snapshot_dataset(&key)
            .await
            .unwrap()
            .is_empty()
    );

    // The first attempt fails, the retry lands: acknowledged.
    fixture.store.fail_wal_puts(1);
    let before = fixture.store.wal_puts();
    fixture
        .worker()
        .write(batch(&fixture.stream, 2_000_000, 4))
        .await
        .unwrap();
    assert_eq!(fixture.store.wal_puts() - before, 2);

    fixture.worker().flush_one(&key).await.unwrap();
    assert_eq!(
        fixture.catalog.committed_rows(),
        vec![4],
        "only the acknowledged rows"
    );
}

#[tokio::test]
async fn a_write_whose_wal_object_lands_after_the_lease_ran_out_is_refused() {
    let fixture = fixture(IntakeSettings::default()).await;
    let buffer = Arc::new(BufferPool::new());
    let short = fixture
        .node_with(buffer.clone(), Duration::from_millis(300))
        .await;
    fixture.store.delay_wal_puts(Duration::from_millis(400));

    let refused = short
        .write(batch(&fixture.stream, 1_000_000, 2))
        .await
        .unwrap_err();

    assert!(refused.to_string().contains("lease"), "{refused}");
    assert_eq!(buffer.reserved_bytes(), 0);
    let key = raw_dataset_key(&fixture).await;
    assert!(buffer.snapshot_dataset(&key).await.unwrap().is_empty());
}

#[tokio::test]
async fn concurrent_writes_share_wal_objects_and_are_all_acknowledged() {
    let fixture = fixture(IntakeSettings::default()).await;
    let key = raw_dataset_key(&fixture).await;
    fixture.store.delay_wal_puts(Duration::from_millis(50));

    // Polled together, so they arrive while the first object is being written.
    let writes = (0..20).map(|offset| {
        fixture
            .worker()
            .write(batch(&fixture.stream, 1_000_000 + offset * 10, 1))
    });

    for write in futures::future::join_all(writes).await {
        write.unwrap();
    }

    let written = objects(&fixture.store, "wal/").await;
    assert!(written < 20, "grouped into fewer objects, got {written}");

    fixture.worker().flush_one(&key).await.unwrap();
    assert_eq!(fixture.catalog.committed_rows(), vec![20]);
    assert_eq!(objects(&fixture.store, "wal/").await, 0);
}

/// The steps of the flush loop's drain branch.
async fn drain(worker: &IntakeWorker) {
    worker
        .flush_keys(worker.buffer.snapshot_keys(), true, "drain")
        .await;
    worker.wal.stop_acknowledging(Duration::from_secs(1)).await;
    worker
        .flush_keys(worker.buffer.snapshot_keys(), true, "drain_final")
        .await;
    worker.release_wal().await;
}

#[tokio::test]
async fn draining_publishes_everything_and_leaves_no_wal_behind() {
    let fixture = fixture(IntakeSettings::default()).await;
    let a = fixture.worker().clone();
    let epoch = a.wal().lease().epoch();
    a.write(batch(&fixture.stream, 1_000_000, 2)).await.unwrap();
    a.write(batch(&fixture.stream, 2_000_000, 2)).await.unwrap();

    drain(&a).await;

    assert_eq!(fixture.catalog.committed_rows(), vec![4]);
    assert_eq!(objects(&fixture.store, "wal/").await, 0);
    assert!(fixture.registry.streams().is_empty());
    assert!(
        fixture.registry.is_fenced(epoch),
        "a drained writer acknowledges nothing more"
    );
    assert!(
        a.write(batch(&fixture.stream, 3_000_000, 1)).await.is_err(),
        "writes after the drain are refused"
    );

    let b = fixture.node().await;
    assert_eq!(
        b.take_over_lapsed(&solo()).await.unwrap(),
        TakeoverRound::default()
    );
    assert_eq!(
        fixture.registry.writer_count(),
        1,
        "the drained writer is forgotten"
    );
}

#[tokio::test]
async fn draining_with_a_failing_catalog_hands_the_rest_over_without_waiting() {
    let fixture = fixture(IntakeSettings::default()).await;
    let a = fixture.worker().clone();
    a.write(batch(&fixture.stream, 1_000_000, 2)).await.unwrap();

    // Both drain flushes fail.
    fixture.catalog.fail_commit_after(0);
    fixture.catalog.fail_next_commit();
    drain(&a).await;
    assert!(fixture.catalog.committed_rows().is_empty());

    // No lease to wait for: the release fenced the writer.
    let b = fixture.node().await;
    let round = b.take_over_lapsed(&solo()).await.unwrap();
    assert_eq!((round.fenced, round.streams, round.rows), (0, 1, 2));
    assert_eq!(fixture.catalog.committed_rows(), vec![2]);
    assert_eq!(objects(&fixture.store, "wal/").await, 0);
}
