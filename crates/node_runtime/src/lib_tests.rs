use std::io::Write as _;

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use super::extract_zip;

/// The zip crate is built with only the codecs we need; make sure a Deflate archive (what
/// Node.js, rust-analyzer and clangd publish) still round-trips through `extract_zip`.
#[test]
fn extract_zip_reads_deflate_archives() {
    let contents = "console.log('hello from node');\n".repeat(64);

    let mut archive = Vec::new();
    {
        let mut writer = ZipWriter::new(std::io::Cursor::new(&mut archive));
        let options = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        writer.start_file("node/bin/hello.js", options).unwrap();
        writer.write_all(contents.as_bytes()).unwrap();
        writer.finish().unwrap();
    }
    assert!(
        archive.len() < contents.len(),
        "the entry should have been compressed"
    );

    let dest = tempfile::tempdir().unwrap();
    futures::executor::block_on(extract_zip(&archive, dest.path(), None::<fn(&str) -> bool>))
        .unwrap();

    let extracted = std::fs::read_to_string(dest.path().join("node/bin/hello.js")).unwrap();
    assert_eq!(extracted, contents);
}
