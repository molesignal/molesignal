// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! The rows in their Parquet file of the documents a query matches: the query reads only the pages
//! those rows are in, not the whole file.

use std::collections::{HashMap, HashSet, hash_map::Entry};

use anyhow::{Result, anyhow};
use tantivy::{
    DocAddress, Searcher, TantivyDocument,
    collector::DocSetCollector,
    columnar::Column,
    schema::{Field, Value},
};

use super::{IndexHandle, ROW_FIELD, ROWS_VERSION};

impl IndexHandle {
    /// The rows in its file of the documents whose fields each hold one of their values in `sets`
    /// (`field = 'value'`, `field IN (...)`), smallest first. `None` when more than `max_rows`
    /// match, where reading that many rows costs more than scanning the file, and when there are
    /// no conditions, the archive has no rows (format 1) or a field is not indexed by whole values
    /// in it. The rows are a fast field, one column read for each segment with matches; archives
    /// written before keep them in the document store, a block read for each match. Call it on a
    /// blocking thread, see [`Self::count_term`].
    pub fn matching_rows(
        &self,
        sets: &[(String, Vec<String>)],
        max_rows: usize,
    ) -> Result<Option<Vec<u64>>> {
        if sets.is_empty() || self.format_version() < ROWS_VERSION {
            return Ok(None);
        }

        let (Some(query), Ok(row)) = (
            self.whole_value_query(sets),
            self.schema.get_field(ROW_FIELD),
        ) else {
            return Ok(None);
        };

        let searcher = self.searcher()?;

        let matches = query
            .count(&searcher)
            .map_err(|e| anyhow!("tantivy count: {e}"))?;

        if matches > max_rows {
            return Ok(None);
        }

        let addresses = searcher
            .search(query.as_ref(), &DocSetCollector)
            .map_err(|e| anyhow!("tantivy rows: {e}"))?;

        let rows = match self.schema.get_field_entry(row).is_fast() {
            true => column_rows(&searcher, addresses)?,
            false => stored_rows(&searcher, row, addresses)?,
        };

        Ok(rows.map(|mut rows| {
            rows.sort_unstable();
            rows
        }))
    }
}

/// The rows of `addresses` from the row column of their segments. `None` when a document has no
/// row: the rows it is in are not known, and a selection would leave it out.
fn column_rows(searcher: &Searcher, addresses: HashSet<DocAddress>) -> Result<Option<Vec<u64>>> {
    let mut columns: HashMap<u32, Option<Column<u64>>> = HashMap::new();
    let mut rows = Vec::with_capacity(addresses.len());

    for address in addresses {
        let column = match columns.entry(address.segment_ord) {
            Entry::Occupied(column) => column.into_mut(),
            Entry::Vacant(slot) => slot.insert(
                searcher
                    .segment_reader(address.segment_ord)
                    .fast_fields()
                    .column_opt::<u64>(ROW_FIELD)
                    .map_err(|e| anyhow!("tantivy row column: {e}"))?,
            ),
        };

        let Some(row) = column
            .as_ref()
            .and_then(|column| column.first(address.doc_id))
        else {
            return Ok(None);
        };

        rows.push(row);
    }

    Ok(Some(rows))
}

/// The rows of `addresses` kept in the document store, as archives written before keep them.
/// `None` when a document has no row.
fn stored_rows(
    searcher: &Searcher,
    row: Field,
    addresses: HashSet<DocAddress>,
) -> Result<Option<Vec<u64>>> {
    let mut rows = Vec::with_capacity(addresses.len());

    for address in addresses {
        let doc: TantivyDocument = searcher
            .doc(address)
            .map_err(|e| anyhow!("tantivy doc: {e}"))?;

        let Some(value) = doc.get_first(row).and_then(|value| value.as_u64()) else {
            return Ok(None);
        };

        rows.push(value);
    }

    Ok(Some(rows))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use object_store::{ObjectStore, ObjectStoreExt, memory::InMemory, path::Path as ObjectPath};
    use tantivy::{
        Index, IndexSettings, IndexWriter,
        schema::{FAST, STORED, STRING, Schema},
    };

    use super::{
        super::{
            FORMAT_PROPERTY, OVERLONG_FIELD, PuffinDirWriter, TIMESTAMP_FIELD, TantivyArchiveOpener,
        },
        *,
    };

    /// An archive written before the rows were a fast field keeps them in the document store; its
    /// matches still give their rows.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_rows_of_an_archive_that_stores_them_are_read_from_its_store() {
        let dir = PuffinDirWriter::new().unwrap();
        let mut schema = Schema::builder();
        let level = schema.add_text_field("level", STRING);
        let timestamp = schema.add_i64_field(TIMESTAMP_FIELD, FAST);
        let row = schema.add_u64_field(ROW_FIELD, STORED);
        schema.add_text_field(OVERLONG_FIELD, STRING);

        let index = Index::create(dir.clone(), schema.build(), IndexSettings::default()).unwrap();
        let mut writer: IndexWriter = index.writer_with_num_threads(1, 15_000_000).unwrap();

        for position in 0..12_u64 {
            let mut doc = TantivyDocument::default();
            doc.add_text(level, if position % 3 == 0 { "error" } else { "info" });
            doc.add_i64(timestamp, position as i64);
            doc.add_u64(row, position);
            writer.add_document(doc).unwrap();
        }

        writer.commit().unwrap();
        writer.wait_merging_threads().unwrap();
        drop(index);
        dir.set_property(FORMAT_PROPERTY, "4");

        let bytes = dir.to_puffin_bytes().unwrap();
        let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());
        let path = ObjectPath::from("stored-rows.ttv");
        store.put(&path, bytes.clone().into()).await.unwrap();

        let handle = TantivyArchiveOpener::open_from_object_store(store, path, bytes.len() as u64)
            .await
            .unwrap();

        let sets = [("level".to_string(), vec!["error".to_string()])];

        let found = tokio::task::spawn_blocking(move || {
            (
                handle.matching_rows(&sets, 100).unwrap(),
                handle.matching_rows(&sets, 3).unwrap(),
            )
        })
        .await
        .unwrap();

        assert_eq!(found, (Some(vec![0, 3, 6, 9]), None));
    }
}
