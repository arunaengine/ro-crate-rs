//! RO-Crate to RDF export example.
//!
//! Demonstrates `ConversionOptions` for handling relative IRIs:
//! - `Strict` (default): Fails on unresolvable relative IRIs
//! - `WithBase`: Resolves relative IRIs against a base (recommended)
//! - `AllowRelative`: Passes IRIs through unresolved
//!
//! Run the demo: `cargo run --example rdf_export --features rdf`
//! Export a crate: `cargo run --example rdf_export --features rdf -- <crate> --format turtle --base <iri>`

#[cfg(feature = "rdf")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    if let Some(input) = args.next() {
        return export_crate(input, args);
    }

    run_demo()
}

#[cfg(feature = "rdf")]
fn export_crate(
    input: String,
    mut args: impl Iterator<Item = String>,
) -> Result<(), Box<dyn std::error::Error>> {
    use rocraters::ro_crate::rdf::{
        ContextResolverBuilder, ConversionOptions, RdfFormat, rocrate_to_rdf_with_options,
    };
    use rocraters::ro_crate::read::read_crate;
    use std::io::{Error, ErrorKind};
    use std::path::PathBuf;

    let mut format = RdfFormat::Turtle;
    let mut base = None;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--format" => {
                let value = args.next().ok_or_else(|| {
                    Error::new(ErrorKind::InvalidInput, "--format requires a value")
                })?;
                format = match value.as_str() {
                    "turtle" => RdfFormat::Turtle,
                    "ntriples" => RdfFormat::NTriples,
                    "nquads" => RdfFormat::NQuads,
                    "rdfxml" => RdfFormat::RdfXml,
                    _ => {
                        return Err(Error::new(
                            ErrorKind::InvalidInput,
                            format!("unsupported RDF format: {value}"),
                        )
                        .into());
                    }
                };
            }
            "--base" => {
                base = Some(args.next().ok_or_else(|| {
                    Error::new(ErrorKind::InvalidInput, "--base requires a value")
                })?);
            }
            _ => {
                return Err(Error::new(
                    ErrorKind::InvalidInput,
                    format!("unknown argument: {argument}"),
                )
                .into());
            }
        }
    }

    let rocrate = read_crate(&PathBuf::from(input), 0)?;
    let options = base.map(ConversionOptions::with_base).unwrap_or_default();
    let rdf_graph =
        rocrate_to_rdf_with_options(&rocrate, ContextResolverBuilder::default(), options)?;
    print!("{}", rdf_graph.to_string(format)?);

    Ok(())
}

#[cfg(feature = "rdf")]
fn run_demo() -> Result<(), Box<dyn std::error::Error>> {
    use rocraters::ro_crate::constraints::{DataType, Id, License};
    use rocraters::ro_crate::metadata_descriptor::MetadataDescriptor;
    use rocraters::ro_crate::rdf::{
        ContextResolverBuilder, ConversionOptions, RdfFormat, rocrate_to_rdf_with_options,
    };
    use rocraters::ro_crate::rocrate::{GraphVector, RoCrate, RoCrateContext};
    use rocraters::ro_crate::root::RootDataEntity;

    // Create a sample RO-Crate
    let rocrate = RoCrate {
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
                name: "Example Research Dataset".to_string(),
                description: "A dataset demonstrating RDF export capabilities".to_string(),
                date_published: "2025-01-15".to_string(),
                license: License::Id(Id::Id("https://spdx.org/licenses/MIT".to_string())),
                dynamic_entity: None,
            }),
        ],
    };

    // WithBase: resolves relative IRIs against the provided base (recommended)
    println!("=== Option 1: WithBase (Recommended) ===\n");

    let resolver = ContextResolverBuilder::default();
    let rdf_graph = rocrate_to_rdf_with_options(
        &rocrate,
        resolver,
        ConversionOptions::with_base("https://example.org/crate/"),
    )?;

    println!("Generated {} triples\n", rdf_graph.len());

    println!("--- Turtle Format ---");
    let turtle = rdf_graph.to_string(RdfFormat::Turtle)?;
    println!("{}\n", turtle);

    println!("--- N-Triples Format ---");
    let ntriples = rdf_graph.to_string(RdfFormat::NTriples)?;
    println!("{}\n", ntriples);

    // Strict: fails on unresolvable relative IRIs (default behavior)
    println!("=== Option 2: Strict (Default) ===\n");

    let resolver = ContextResolverBuilder::default();
    let strict_result = rocrate_to_rdf_with_options(&rocrate, resolver, ConversionOptions::Strict);

    match strict_result {
        Ok(graph) => println!("Success: {} triples\n", graph.len()),
        Err(e) => println!("Expected error (no @base defined): {}\n", e),
    }

    // AllowRelative: passes relative IRIs through unresolved (may produce invalid RDF)
    println!("=== Option 3: AllowRelative ===\n");

    let resolver = ContextResolverBuilder::default();
    let rdf_allow_relative = rocrate_to_rdf_with_options(
        &rocrate, // Same crate with relative IRIs
        resolver,
        ConversionOptions::AllowRelative,
    );

    match rdf_allow_relative {
        Ok(graph) => {
            println!(
                "AllowRelative: {} triples (relative IRIs unresolved)",
                graph.len()
            );
            let turtle = graph.to_string(RdfFormat::Turtle)?;
            println!("{}\n", turtle);
        }
        Err(e) => println!("AllowRelative failed: {}\n", e),
    }

    // Same crate with a different base IRI
    println!("=== Different Base IRI ===\n");
    let resolver = ContextResolverBuilder::default();
    let rdf_with_base = rocrate_to_rdf_with_options(
        &rocrate,
        resolver,
        ConversionOptions::WithBase("https://example.org/my-research/".to_string()),
    )?;
    let turtle_with_base = rdf_with_base.to_string(RdfFormat::Turtle)?;
    println!("{}", turtle_with_base);

    Ok(())
}

#[cfg(not(feature = "rdf"))]
fn main() {
    eprintln!("This example requires the 'rdf' feature.");
    eprintln!("Run with: cargo run --example rdf_export --features rdf");
    std::process::exit(1);
}
