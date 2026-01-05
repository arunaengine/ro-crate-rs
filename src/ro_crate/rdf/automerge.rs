//! Automerge integration for RDF graphs
//!
//! This module provides CRDT-based synchronization for RDF graphs using Automerge.
//! It enables offline-first sync, distributed reconciliation, and version control
//! workflows for RO-Crates.

use std::collections::{BTreeMap, BTreeSet, HashSet};

use automerge::sync::SyncDoc;
use automerge::AutoCommit;
use autosurgeon::{hydrate as am_hydrate, reconcile as am_reconcile, Hydrate, Reconcile};
use oxrdf::Triple;
use thiserror::Error;

use super::{RdfGraph, ResolvedContext};

/// RDF graph represented as entity-grouped triples for Automerge CRDT sync.
///
/// Structure: `BTreeMap<EntityId, BTreeMap<Predicate, Vec<Object>>>`
///
/// Uses BTree collections for deterministic ordering, ensuring consistent
/// serialization across platforms. Uses Vec for objects since autosurgeon
/// doesn't implement Reconcile/Hydrate for BTreeSet.
#[derive(Reconcile, Hydrate, Clone, Debug, PartialEq, Default)]
pub struct AutomergeRdfGraph {
    /// Entities grouped by subject IRI (N-Triples format string)
    pub entities: BTreeMap<String, BTreeMap<String, Vec<String>>>,

    /// JSON-serialized ResolvedContext as bytes (opaque to avoid text CRDT semantics)
    pub context: Vec<u8>,
}

#[derive(Debug, Error)]
pub enum AutomergeError {
    #[error("Automerge error: {0}")]
    Automerge(#[from] automerge::AutomergeError),

    #[error("Hydrate error: {0}")]
    Hydrate(#[from] autosurgeon::HydrateError),

    #[error("Reconcile error: {0}")]
    Reconcile(#[from] autosurgeon::ReconcileError),

    #[error("Context serialization error: {0}")]
    ContextSerialization(#[from] serde_json::Error),

    #[error("Invalid RDF term: {0}")]
    InvalidTerm(String),
}

/// Convert an RdfGraph to an AutomergeRdfGraph for CRDT sync
pub fn rdf_to_automerge(graph: &RdfGraph) -> Result<AutomergeRdfGraph, AutomergeError> {
    // Use BTreeSet internally for deduplication, then convert to Vec
    let mut entities_set: BTreeMap<String, BTreeMap<String, BTreeSet<String>>> = BTreeMap::new();

    for triple in graph.triples.iter() {
        // Use each field's Display impl directly (N-Triples format)
        let subject = triple.subject.to_string();
        let predicate = triple.predicate.to_string();
        let object = triple.object.to_string();

        entities_set
            .entry(subject)
            .or_default()
            .entry(predicate)
            .or_default()
            .insert(object);
    }

    // Convert BTreeSet to Vec for autosurgeon compatibility
    let entities: BTreeMap<String, BTreeMap<String, Vec<String>>> = entities_set
        .into_iter()
        .map(|(subj, preds)| {
            let preds_vec: BTreeMap<String, Vec<String>> = preds
                .into_iter()
                .map(|(pred, objs)| (pred, objs.into_iter().collect()))
                .collect();
            (subj, preds_vec)
        })
        .collect();

    let context = serde_json::to_vec(&graph.context)?;

    Ok(AutomergeRdfGraph { entities, context })
}

/// Convert an AutomergeRdfGraph back to an RdfGraph
pub fn automerge_to_rdf(graph: &AutomergeRdfGraph) -> Result<RdfGraph, AutomergeError> {
    let context: ResolvedContext = serde_json::from_slice(&graph.context)?;
    let mut triples = HashSet::new();

    for (subject, predicates) in &graph.entities {
        for (predicate, objects) in predicates {
            for object in objects {
                // Reconstruct N-Triples line and parse using Triple::from_str
                let triple_str = format!("{} {} {} .", subject, predicate, object);
                let triple: Triple = triple_str.parse().map_err(|e: oxrdf::TermParseError| {
                    AutomergeError::InvalidTerm(format!("{}", e))
                })?;
                triples.insert(triple);
            }
        }
    }

    Ok(RdfGraph { triples, context })
}

/// Create a new Automerge document from an AutomergeRdfGraph
pub fn new_document(graph: &AutomergeRdfGraph) -> Result<AutoCommit, AutomergeError> {
    let mut doc = AutoCommit::new();
    am_reconcile(&mut doc, graph)?;
    Ok(doc)
}

/// Extract an AutomergeRdfGraph from an Automerge document
pub fn hydrate(doc: &AutoCommit) -> Result<AutomergeRdfGraph, AutomergeError> {
    Ok(am_hydrate(doc)?)
}

/// Update an Automerge document to match the given graph
pub fn reconcile(doc: &mut AutoCommit, graph: &AutomergeRdfGraph) -> Result<(), AutomergeError> {
    Ok(am_reconcile(doc, graph)?)
}

/// Save an Automerge document to bytes
pub fn save(doc: &mut AutoCommit) -> Vec<u8> {
    doc.save()
}

/// Load an Automerge document from bytes
pub fn load(bytes: &[u8]) -> Result<AutoCommit, AutomergeError> {
    Ok(AutoCommit::load(bytes)?)
}

/// Fork a document for independent editing
pub fn fork(doc: &mut AutoCommit) -> AutoCommit {
    doc.fork()
}

/// Merge another document into this one
pub fn merge(doc: &mut AutoCommit, other: &mut AutoCommit) -> Result<(), AutomergeError> {
    let _ = doc.merge(other)?;
    Ok(())
}

/// Create a new sync state for tracking synchronization with a peer.
pub fn new_sync_state() -> automerge::sync::State {
    automerge::sync::State::new()
}

/// Get the current heads (latest changes) of the document.
pub fn get_heads(doc: &mut AutoCommit) -> Vec<automerge::ChangeHash> {
    doc.get_heads()
}

/// Generate a sync message to send to a peer.
pub fn generate_sync_message(
    doc: &mut AutoCommit,
    sync_state: &mut automerge::sync::State,
) -> Option<automerge::sync::Message> {
    doc.sync().generate_sync_message(sync_state)
}

/// Receive and apply a sync message from a peer.
pub fn receive_sync_message(
    doc: &mut AutoCommit,
    sync_state: &mut automerge::sync::State,
    message: automerge::sync::Message,
) -> Result<(), AutomergeError> {
    doc.sync().receive_sync_message(sync_state, message)?;
    Ok(())
}

/// Perform a full sync between two documents.
pub fn sync_documents(
    doc_a: &mut AutoCommit,
    doc_b: &mut AutoCommit,
) -> Result<(), AutomergeError> {
    let mut state_a = new_sync_state();
    let mut state_b = new_sync_state();

    loop {
        let msg_a = generate_sync_message(doc_a, &mut state_a);
        let msg_b = generate_sync_message(doc_b, &mut state_b);

        if msg_a.is_none() && msg_b.is_none() {
            break;
        }

        if let Some(msg) = msg_a {
            receive_sync_message(doc_b, &mut state_b, msg)?;
        }
        if let Some(msg) = msg_b {
            receive_sync_message(doc_a, &mut state_a, msg)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ro_crate::read::read_crate;
    use crate::ro_crate::rdf::{rocrate_to_rdf_with_options, ConversionOptions, ContextResolverBuilder};
    use std::path::{Path, PathBuf};

    const TEST_BASE: &str = "http://example.org/";

    fn fixture_path(relative_path: &str) -> PathBuf {
        Path::new("tests/fixtures").join(relative_path)
    }

    fn load_fixture_as_automerge(filename: &str) -> AutomergeRdfGraph {
        let path = fixture_path(filename);
        let crate_ = read_crate(&path, 0).expect("Failed to read fixture");
        let resolver = ContextResolverBuilder::default();
        let options = ConversionOptions::with_base(TEST_BASE);
        let rdf_graph = rocrate_to_rdf_with_options(&crate_, resolver, options)
            .expect("Failed to convert to RDF");
        rdf_to_automerge(&rdf_graph).expect("Failed to convert to Automerge")
    }

    #[test]
    fn test_roundtrip_rdf_to_automerge() {
        let path = fixture_path("_ro-crate-metadata-minimal.json");
        let crate_ = read_crate(&path, 0).expect("Failed to read fixture");
        let resolver = ContextResolverBuilder::default();
        let options = ConversionOptions::with_base(TEST_BASE);
        let rdf_graph = rocrate_to_rdf_with_options(&crate_, resolver, options)
            .expect("Failed to convert to RDF");

        let am_graph = rdf_to_automerge(&rdf_graph).unwrap();
        let roundtrip_rdf = automerge_to_rdf(&am_graph).unwrap();

        assert_eq!(rdf_graph.triples.len(), roundtrip_rdf.triples.len());
        for triple in &rdf_graph.triples {
            assert!(roundtrip_rdf.triples.contains(triple));
        }
    }

    #[test]
    fn test_document_roundtrip() {
        let am_graph = load_fixture_as_automerge("_ro-crate-metadata-minimal.json");

        let mut doc = new_document(&am_graph).unwrap();
        let bytes = save(&mut doc);
        let loaded_doc = load(&bytes).unwrap();
        let hydrated = hydrate(&loaded_doc).unwrap();

        assert_eq!(am_graph, hydrated);
    }

    #[test]
    fn test_fork_and_merge() {
        let am_graph = load_fixture_as_automerge("_ro-crate-metadata-minimal.json");

        let mut doc_a = new_document(&am_graph).unwrap();
        let mut doc_b = fork(&mut doc_a);

        // Add new entity on fork A
        let mut graph_a = hydrate(&doc_a).unwrap();
        graph_a
            .entities
            .entry("<http://example.org/new-file-a>".to_string())
            .or_default()
            .entry("<http://schema.org/name>".to_string())
            .or_default()
            .push("\"File from A\"".to_string());
        reconcile(&mut doc_a, &graph_a).unwrap();

        // Add new entity on fork B
        let mut graph_b = hydrate(&doc_b).unwrap();
        graph_b
            .entities
            .entry("<http://example.org/new-file-b>".to_string())
            .or_default()
            .entry("<http://schema.org/name>".to_string())
            .or_default()
            .push("\"File from B\"".to_string());
        reconcile(&mut doc_b, &graph_b).unwrap();

        // Merge preserves both additions
        merge(&mut doc_a, &mut doc_b).unwrap();
        let merged = hydrate(&doc_a).unwrap();

        assert!(merged
            .entities
            .contains_key("<http://example.org/new-file-a>"));
        assert!(merged
            .entities
            .contains_key("<http://example.org/new-file-b>"));
    }

    #[test]
    fn test_sync_protocol() {
        let am_graph = load_fixture_as_automerge("_ro-crate-metadata-minimal.json");

        let mut doc_a = new_document(&am_graph).unwrap();
        let mut doc_b = fork(&mut doc_a);

        // Make changes on each fork
        let mut graph_a = hydrate(&doc_a).unwrap();
        graph_a
            .entities
            .entry("<http://example.org/sync-a>".to_string())
            .or_default()
            .entry("<http://schema.org/name>".to_string())
            .or_default()
            .push("\"Synced A\"".to_string());
        reconcile(&mut doc_a, &graph_a).unwrap();

        let mut graph_b = hydrate(&doc_b).unwrap();
        graph_b
            .entities
            .entry("<http://example.org/sync-b>".to_string())
            .or_default()
            .entry("<http://schema.org/name>".to_string())
            .or_default()
            .push("\"Synced B\"".to_string());
        reconcile(&mut doc_b, &graph_b).unwrap();

        sync_documents(&mut doc_a, &mut doc_b).unwrap();

        let synced_a = hydrate(&doc_a).unwrap();
        let synced_b = hydrate(&doc_b).unwrap();

        // Both documents converge after sync
        assert!(synced_a.entities.contains_key("<http://example.org/sync-a>"));
        assert!(synced_a.entities.contains_key("<http://example.org/sync-b>"));
        assert!(synced_b.entities.contains_key("<http://example.org/sync-a>"));
        assert!(synced_b.entities.contains_key("<http://example.org/sync-b>"));
    }
}
