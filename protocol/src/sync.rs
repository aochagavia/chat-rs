use std::io::{self, Read, Write};

pub fn write_message<W: Write>(mut writer: W, msg: &str) -> io::Result<()> {
    if msg.len() > 255 {
        return Err(io::Error::new(io::ErrorKind::InvalidData,
                                  "message is longer than 255 bytes"));
    }

    let len = msg.len() as u8;
    writer.write_all(&[len])?;
    writer.write_all(msg.as_bytes())
}

pub fn read_message<R: Read>(reader: &mut R) -> io::Result<String> {
    let mut len = [0];
    reader.read_exact(&mut len)?;

    let mut buffer = vec![0; len[0] as usize];
    reader.read_exact(&mut buffer)?;

    String::from_utf8(buffer)
           .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Invalid utf-8"))
}
