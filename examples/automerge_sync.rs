//! Example: Synchronizing RO-Crates with Automerge
//!
//! This example demonstrates:
//! 1. Converting an RO-Crate to an Automerge document
//! 2. Forking for independent editing
//! 3. Making divergent changes
//! 4. Merging changes back together
//! 5. Converting the merged result back to a valid RO-Crate
//!
//! Run with: cargo run --example automerge_sync --features automerge

#[cfg(feature = "automerge")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    use rocraters::ro_crate::constraints::{DataType, Id, License};
    use rocraters::ro_crate::metadata_descriptor::MetadataDescriptor;
    use rocraters::ro_crate::rdf::automerge::{
        automerge_to_rdf, fork, hydrate, load, merge, new_document, rdf_to_automerge, reconcile,
        save, sync_documents,
    };
    use rocraters::ro_crate::rdf::{
        rdf_graph_to_rocrate, rocrate_to_rdf_with_options, ContextResolverBuilder,
        ConversionOptions,
    };
    use rocraters::ro_crate::rocrate::{GraphVector, RoCrate, RoCrateContext};
    use rocraters::ro_crate::root::RootDataEntity;

    println!("=== RO-Crate Automerge Sync Example ===\n");

    // Step 1: Create an RO-Crate and convert to Automerge
    println!("1. Creating initial RO-Crate...");

    // Create a properly structured RO-Crate with metadata descriptor and root entity
    let crate_ = RoCrate {
        context: RoCrateContext::ReferenceContext(
            "https://w3id.org/ro/crate/1.2/context".to_string(),
        ),
        graph: vec![
            GraphVector::MetadataDescriptor(MetadataDescriptor {
                id: "ro-crate-metadata.json".to_string(),
                type_: DataType::Term("CreativeWork".to_string()),
                conforms_to: Id::Id("https://w3id.org/ro/crate/1.2".to_string()),
                about: Id::Id("./".to_string()),
                dynamic_entity: None,
            }),
            GraphVector::RootDataEntity(RootDataEntity {
                id: "./".to_string(),
                type_: DataType::Term("Dataset".to_string()),
                name: "Collaborative Research Dataset".to_string(),
                description: "A dataset demonstrating collaborative editing with Automerge"
                    .to_string(),
                date_published: "2025-01-15".to_string(),
                license: License::Id(Id::Id("https://spdx.org/licenses/MIT".to_string())),
                dynamic_entity: None,
            }),
        ],
    };

    let resolver = ContextResolverBuilder::default();
    let rdf_graph = rocrate_to_rdf_with_options(
        &crate_,
        resolver,
        ConversionOptions::WithBase("http://example.org/crate/".to_string()),
    )?;
    let am_graph = rdf_to_automerge(&rdf_graph)?;
    let mut doc_main = new_document(&am_graph)?;

    println!("   Initial entities: {}", am_graph.entities.len());
    println!(
        "   Initial triples in RDF graph: {}",
        rdf_graph.triples.len()
    );

    // Step 2: Fork for independent editing (simulating two users)
    println!("\n2. Forking document for User A and User B...");
    let mut doc_user_a = fork(&mut doc_main);
    let mut doc_user_b = fork(&mut doc_main);

    // Step 3: User A makes changes
    println!("\n3. User A adds a new data entity...");
    let mut graph_a = hydrate(&doc_user_a)?;
    graph_a
        .entities
        .entry("<http://example.org/data/file1.csv>".to_string())
        .or_default()
        .entry("<http://www.w3.org/1999/02/22-rdf-syntax-ns#type>".to_string())
        .or_default()
        .push("<http://schema.org/MediaObject>".to_string());
    graph_a
        .entities
        .get_mut("<http://example.org/data/file1.csv>")
        .unwrap()
        .entry("<http://schema.org/name>".to_string())
        .or_default()
        .push("\"Experiment Data\"".to_string());
    reconcile(&mut doc_user_a, &graph_a)?;

    // Step 4: User B makes different changes (concurrently)
    println!("   User B adds a contextual entity...");
    let mut graph_b = hydrate(&doc_user_b)?;
    graph_b
        .entities
        .entry("<http://example.org/author1>".to_string())
        .or_default()
        .entry("<http://www.w3.org/1999/02/22-rdf-syntax-ns#type>".to_string())
        .or_default()
        .push("<http://schema.org/Person>".to_string());
    graph_b
        .entities
        .get_mut("<http://example.org/author1>")
        .unwrap()
        .entry("<http://schema.org/name>".to_string())
        .or_default()
        .push("\"Alice Researcher\"".to_string());
    reconcile(&mut doc_user_b, &graph_b)?;

    // Step 5: Merge changes
    println!("\n4. Merging User A and User B changes...");
    merge(&mut doc_user_a, &mut doc_user_b)?;
    let merged_graph = hydrate(&doc_user_a)?;

    println!("   Merged entities: {}", merged_graph.entities.len());
    println!(
        "   Contains file1.csv: {}",
        merged_graph
            .entities
            .contains_key("<http://example.org/data/file1.csv>")
    );
    println!(
        "   Contains author1: {}",
        merged_graph
            .entities
            .contains_key("<http://example.org/author1>")
    );

    // Step 6: Convert back to RO-Crate
    println!("\n5. Converting merged document back to RO-Crate...");
    let merged_rdf = automerge_to_rdf(&merged_graph)?;
    let merged_crate = rdf_graph_to_rocrate(merged_rdf)?;

    println!("   Final RO-Crate entities: {}", merged_crate.graph.len());

    // Step 7: Demonstrate persistence
    println!("\n6. Saving and loading Automerge document...");
    let bytes = save(&mut doc_user_a);
    println!("   Saved document size: {} bytes", bytes.len());

    let loaded_doc = load(&bytes)?;
    let loaded_graph = hydrate(&loaded_doc)?;
    println!("   Loaded entities: {}", loaded_graph.entities.len());
    println!("   Roundtrip successful: {}", loaded_graph == merged_graph);

    // Step 8: Demonstrate sync protocol
    // The sync protocol enables bidirectional sync between forked documents
    // over a network, without needing to pass the full document state.
    println!("\n7. Demonstrating sync protocol...");
    let mut sync_doc_main = new_document(&am_graph)?;
    let mut sync_doc_a = fork(&mut sync_doc_main);
    let mut sync_doc_b = fork(&mut sync_doc_main);

    // Make different changes on each fork (simulating offline edits)
    let mut g_a = hydrate(&sync_doc_a)?;
    g_a.entities
        .entry("<http://example.org/sync-a>".to_string())
        .or_default()
        .entry("<http://schema.org/name>".to_string())
        .or_default()
        .push("\"From Sync A\"".to_string());
    reconcile(&mut sync_doc_a, &g_a)?;

    let mut g_b = hydrate(&sync_doc_b)?;
    g_b.entities
        .entry("<http://example.org/sync-b>".to_string())
        .or_default()
        .entry("<http://schema.org/name>".to_string())
        .or_default()
        .push("\"From Sync B\"".to_string());
    reconcile(&mut sync_doc_b, &g_b)?;

    // Sync the documents using the sync protocol
    sync_documents(&mut sync_doc_a, &mut sync_doc_b)?;

    let synced = hydrate(&sync_doc_a)?;
    println!(
        "   After sync, doc A has entity from B: {}",
        synced.entities.contains_key("<http://example.org/sync-b>")
    );

    println!("\n=== Example complete! ===");
    Ok(())
}

#[cfg(not(feature = "automerge"))]
fn main() {
    eprintln!("This example requires the 'automerge' feature.");
    eprintln!("Run with: cargo run --example automerge_sync --features automerge");
    std::process::exit(1);
}
