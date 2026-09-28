// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

use super::*;

#[test]
fn archive_key_date_extracts_day_segment() {
    assert_eq!(
        archive_key_date("blobs/v1/org1/profiles/api/cpu/20260618/abc.pprof.zst"),
        Some("20260618")
    );
    // non-8-digit date segment → None
    assert_eq!(
        archive_key_date("blobs/v1/o/profiles/s/t/notadate/x.pprof.zst"),
        None
    );
    // unversioned legacy paths are deliberately not interpreted.
    assert_eq!(
        archive_key_date("profiles/org1/api/cpu/20260618/abc.pprof.zst"),
        None
    );
    assert_eq!(
        archive_key_date("blobs/v2/org1/profiles/api/cpu/20260618/abc.pprof.zst"),
        None
    );
    // too few segments (not an archive key) → None
    assert_eq!(archive_key_date("rum/org1/sess/1.replay"), None);
}

#[tokio::test]
async fn sweep_expired_archives_filters_by_prefix_and_date() {
    use object_store::memory::InMemory;
    let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
    let org = Id::from_string("org1");
    put_archive(
        &store,
        "blobs/v1/org1/profiles/api/cpu/20260101/a.pprof.zst",
        b"x",
    )
    .await
    .unwrap();
    put_archive(
        &store,
        "blobs/v1/org1/profiles/api/cpu/20260620/b.pprof.zst",
        b"y",
    )
    .await
    .unwrap();
    // unrelated prefix is never touched.
    put_archive(
        &store,
        "blobs/v1/org1/rum/app/sess/0000000001-hash.ndjson.zst",
        b"z",
    )
    .await
    .unwrap();

    let other_org_key = "blobs/v1/org10/profiles/api/cpu/20260101/c.pprof.zst";
    put_archive(&store, other_org_key, b"other organization")
        .await
        .unwrap();

    // cutoff at epoch (1970) → nothing is older → nothing deleted.
    assert_eq!(sweep_expired_archives(&store, &org, 0).await.unwrap(), 0);

    // cutoff far in the future → both profiles archives are older → both gone.
    let far_future_micros = 7_258_118_400_000_000;
    assert_eq!(
        sweep_expired_archives(&store, &org, far_future_micros)
            .await
            .unwrap(),
        2
    );
    assert!(
        get_archive(
            &store,
            "blobs/v1/org1/profiles/api/cpu/20260101/a.pprof.zst"
        )
        .await
        .is_err()
    );
    assert!(get_archive(&store, other_org_key).await.is_ok());
    // the unrelated object survives.
    assert!(
        get_archive(
            &store,
            "blobs/v1/org1/rum/app/sess/0000000001-hash.ndjson.zst"
        )
        .await
        .is_ok()
    );
}

#[test]
fn archive_key_layout() {
    let key = archive_object_key(
        &Id::from_string("org123"),
        "checkout svc",
        "cpu",
        1_700_000_000_000_000,
        &Id::from_string("p1"),
    );
    assert_eq!(
        key,
        "blobs/v1/org123/profiles/checkout_svc/cpu/20231114/p1.pprof.zst"
    );
}

#[test]
fn yyyymmdd_is_civil_date() {
    assert_eq!(yyyymmdd(1_700_000_000_000_000), "20231114");
    assert_eq!(yyyymmdd(0), "19700101");
}

#[test]
fn zstd_round_trips() {
    let data = b"the quick brown fox jumps over the lazy dog".repeat(10);
    let c = zstd_compress(&data).unwrap();
    assert_eq!(zstd_decompress(&c).unwrap(), data);
}
