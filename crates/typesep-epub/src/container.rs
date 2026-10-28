use std::io::{Cursor, Write};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

pub struct Container {
    writer: ZipWriter<Cursor<Vec<u8>>>,
}

impl Container {
    pub fn new() -> Result<Self, zip::result::ZipError> {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        writer.start_file(
            "mimetype",
            SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
        )?;
        writer.write_all(b"application/epub+zip")?;
        Ok(Container { writer })
    }

    pub fn add(&mut self, name: &str, bytes: &[u8]) -> Result<(), zip::result::ZipError> {
        self.writer.start_file(
            name,
            SimpleFileOptions::default().compression_method(CompressionMethod::Deflated),
        )?;
        self.writer.write_all(bytes)?;
        Ok(())
    }

    pub fn finish(self) -> Result<Vec<u8>, zip::result::ZipError> {
        Ok(self.writer.finish()?.into_inner())
    }
}
