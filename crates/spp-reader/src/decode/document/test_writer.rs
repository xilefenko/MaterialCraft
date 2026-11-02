//! Test-only writer for small synthetic files in Painter's object serialisation.

pub struct DocWriter {
    pub bytes: Vec<u8>,
    open: Vec<usize>,
}

impl DocWriter {
    /// Header with the given format version; the root value comes next.
    pub fn new(version: u32) -> Self {
        let mut bytes = vec![0xdd, 0x2f, 0x7c, 0x1b, 0, 0, 0, 0];
        bytes.extend_from_slice(&version.to_le_bytes());
        DocWriter {
            bytes,
            open: Vec::new(),
        }
    }

    fn u32(&mut self, v: u32) -> &mut Self {
        self.bytes.extend_from_slice(&v.to_le_bytes());
        self
    }

    fn string(&mut self, s: &str) -> &mut Self {
        self.u32(s.len() as u32);
        self.bytes.extend_from_slice(s.as_bytes());
        self
    }

    fn end_placeholder(&mut self) {
        self.open.push(self.bytes.len());
        self.u32(0);
    }

    /// Starts a field; the value written next belongs to it.
    pub fn field(&mut self, name: &str) -> &mut Self {
        self.string(name)
    }

    /// Writes raw bytes, e.g. a tag and its payload.
    pub fn raw(&mut self, bytes: &[u8]) -> &mut Self {
        self.bytes.extend_from_slice(bytes);
        self
    }

    /// Opens an object with `tag` (0x12 or 0x14); close it with [`DocWriter::close`].
    pub fn object(&mut self, tag: u8, class: &str, fields: u16) -> &mut Self {
        self.bytes.extend_from_slice(&[tag, 0x00]);
        self.end_placeholder();
        self.string(class);
        self.bytes.extend_from_slice(&fields.to_le_bytes());
        self
    }

    /// Opens a list with `tag` (0x11 or 0x13); close it with [`DocWriter::close`].
    pub fn list(&mut self, tag: u8, count: u32) -> &mut Self {
        self.bytes.push(tag);
        self.end_placeholder();
        self.u32(count)
    }

    /// Closes the innermost open object or list, filling in its end offset.
    pub fn close(&mut self) -> &mut Self {
        if let Some(at) = self.open.pop() {
            let end = (self.bytes.len() as u32).to_le_bytes();
            self.bytes[at..at + 4].copy_from_slice(&end);
        }
        self
    }
}

/// A small document: a root with a bool, a null pointer, a string, and a list of two layers,
/// one of which holds a matrix and an inline value object.
pub fn sample() -> Vec<u8> {
    let mut w = DocWriter::new(78);
    w.object(0x12, "DataDocument", 4);
    w.field("colorManagementACE").raw(&[0x12, 0xff]);
    w.field("enabled").raw(&[0x0a, 0xff]);
    w.field("label").raw(&[0x10, 3, 0, 0, 0]).raw(b"Ink");
    w.field("layers").list(0x13, 2);
    for _ in 0..2 {
        w.object(0x12, "DataLayerColor", 2);
        w.field("viewMatrix").raw(&[0x0e]).raw(&[0; 64]);
        w.field("uvTransformation").object(0x14, "Data2DTransformation", 1);
        w.field("GUIPivot").raw(&[0x02]).raw(&[0; 8]);
        w.close().close();
    }
    w.close().close();
    w.bytes
}
