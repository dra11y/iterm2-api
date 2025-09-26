fn main() -> anyhow::Result<()> {
    println!("cargo:rerun-if-changed=proto/api.proto");
    println!("cargo:rerun-if-changed=build.rs");
    
    protobuf_codegen::Codegen::new()
        .pure()
        .out_dir("src/generated")
        .inputs(&["proto/api.proto"])
        .include("proto")
        .run()?;
    Ok(())
}