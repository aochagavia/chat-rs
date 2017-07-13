use std::fmt::Write;
use std::io::{self, Cursor};

use bytes::BytesMut;
use bytes::buf::BufMut;
use tokio_io::codec::{Encoder, Decoder};
use super::sync;

pub struct ChatCodec;

impl Encoder for ChatCodec {
    type Item = String;
    type Error = io::Error;

    fn encode(&mut self, msg: String, dst: &mut BytesMut) -> Result<(), Self::Error> {
        if dst.len() < msg.len() + 1 {
            let additional = msg.len() + 1 - dst.len();
            dst.reserve(additional);
        }

        if msg.len() > 255 {
            return Err(io::Error::new(io::ErrorKind::InvalidData,
                                    "message is longer than 255 bytes"));
        }

        let len = msg.len() as u8;
        dst.put_u8(len);
        dst.write_str(&msg).expect("Argh");
        Ok(())
    }
}

impl Decoder for ChatCodec {
    type Item = String;
    type Error = io::Error;

    fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<String>, Self::Error> {
        // Without knowing the length of the message, we can't do anything
        if buf.len() == 0 {
            return Ok(None);
        }

        // We add 1 to account for the size of the leading byte
        let len = buf[0] as usize + 1;

        // We can only proceed if the whole message is in the buffer
        if buf.len() < len {
            return Ok(None);
        }

        let msg = sync::read_message(&mut Cursor::new(&*buf));
        buf.split_to(len);

        msg.map(Some)
    }
}
