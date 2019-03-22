#![allow(unused_variables, dead_code)]
use crate::{Error, Res};
use neqo_common::data::*;
use neqo_common::varint::*;
use neqo_crypto::ext::{ExtensionHandler, ExtensionHandlerResult, ExtensionWriterResult};
use neqo_crypto::{HandshakeMessage, TLS_HS_CLIENT_HELLO, TLS_HS_ENCRYPTED_EXTENSIONS};
use std::collections::HashMap;

struct PreferredAddress {
    // TODO(ekr@rtfm.com): Implement.
}

pub mod consts {
    pub const TRANSPORT_PARAMETER_ORIGINAL_CONNECTION_ID: u16 = 0;
    pub const TRANSPORT_PARAMETER_IDLE_TIMEOUT: u16 = 1;
    pub const TRANSPORT_PARAMETER_STATELESS_RESET_TOKEN: u16 = 2;
    pub const TRANSPORT_PARAMETER_MAX_PACKET_SIZE: u16 = 3;
    pub const TRANSPORT_PARAMETER_INITIAL_MAX_DATA: u16 = 4;
    pub const TRANSPORT_PARAMETER_INITIAL_MAX_STREAM_DATA_BIDI_LOCAL: u16 = 5;
    pub const TRANSPORT_PARAMETER_INITIAL_MAX_STREAM_DATA_BIDI_REMOTE: u16 = 6;
    pub const TRANSPORT_PARAMETER_INITIAL_MAX_STREAM_DATA_UNI: u16 = 7;
    pub const TRANSPORT_PARAMETER_INITIAL_MAX_STREAMS_BIDI: u16 = 8;
    pub const TRANSPORT_PARAMETER_INITIAL_MAX_STREAMS_UNI: u16 = 9;
    pub const TRANSPORT_PARAMETER_ACK_DELAY_EXPONENT: u16 = 10;
    pub const TRANSPORT_PARAMETER_MAX_ACK_DELAY: u16 = 11;
    pub const TRANSPORT_PARAMETER_DISABLE_MIGRATION: u16 = 12;
    pub const TRANSPORT_PARAMETER_PREFERRED_ADDRESS: u16 = 13;
}

use consts::*;

#[derive(PartialEq, Debug)]
pub enum TransportParameter {
    Bytes(Vec<u8>),
    Integer(u64),
    Empty,
}

impl TransportParameter {
    fn encode(&self, d: &mut Data, tipe: u16) -> Res<()> {
        d.encode_uint(tipe, 2);
        match self {
            TransportParameter::Bytes(a) => {
                d.encode_uint(a.len() as u64, 2);
                d.encode_vec(a);
            }
            TransportParameter::Integer(a) => {
                d.encode_uint(get_varint_len(*a), 2);
                d.encode_varint(*a);
            }
            TransportParameter::Empty => {
                d.encode_uint(0_u64, 2);
            }
        };

        Ok(())
    }

    fn decode(d: &mut Data) -> Res<(u16, TransportParameter)> {
        let tipe = d.decode_uint(2)? as u16;
        let length = d.decode_uint(2)? as usize;
        let remaining = d.remaining();
        // TODO(ekr@rtfm.com): Sure would be nice to have a version
        // of Data that returned another data that was a slice on
        // this one, so I could check the length more easily.
        let tp = match tipe {
            TRANSPORT_PARAMETER_ORIGINAL_CONNECTION_ID => {
                TransportParameter::Bytes(d.decode_data(length)?)
            }
            TRANSPORT_PARAMETER_STATELESS_RESET_TOKEN => {
                if length != 16 {
                    return Err(Error::TransportParameterError);
                }
                TransportParameter::Bytes(d.decode_data(length)?)
            },
            TRANSPORT_PARAMETER_IDLE_TIMEOUT
            | TRANSPORT_PARAMETER_INITIAL_MAX_DATA
            | TRANSPORT_PARAMETER_INITIAL_MAX_STREAM_DATA_BIDI_LOCAL
            | TRANSPORT_PARAMETER_INITIAL_MAX_STREAM_DATA_BIDI_REMOTE
            | TRANSPORT_PARAMETER_INITIAL_MAX_STREAM_DATA_UNI
            | TRANSPORT_PARAMETER_INITIAL_MAX_STREAMS_BIDI
            | TRANSPORT_PARAMETER_INITIAL_MAX_STREAMS_UNI
            | TRANSPORT_PARAMETER_MAX_ACK_DELAY => TransportParameter::Integer(d.decode_varint()?),

            TRANSPORT_PARAMETER_MAX_PACKET_SIZE => {
                let tmp = d.decode_varint()?;
                if tmp < 1200 {
                    return Err(Error::TransportParameterError);
                }
                TransportParameter::Integer(tmp)
            },
            TRANSPORT_PARAMETER_ACK_DELAY_EXPONENT => {
                let tmp = d.decode_varint()?;
                if tmp > 20 {
                    return Err(Error::TransportParameterError);
                }
                TransportParameter::Integer(tmp)
            }
            ,
            // Skip.
            // TODO(ekr@rtfm.com): Write a skip.
            _ => {
                d.decode_data(length as usize)?;
                return Err(Error::UnknownTransportParameter);
            }
        };

        // Check that we consumed the right amount.
        if (remaining - d.remaining()) > length {
            return Err(Error::NoMoreData);
        }
        if (remaining - d.remaining()) > length {
            return Err(Error::TooMuchData);
        }

        Ok((tipe, tp))
    }
}

#[derive(Default, PartialEq, Debug)]
pub struct TransportParameters {
    params: HashMap<u16, TransportParameter>,
}

impl TransportParameters {
    pub fn encode(&self, d: &mut Data) -> Res<()> {
        for (tipe, tp) in &self.params {
            tp.encode(d, *tipe)?;
        }
        Ok(())
    }

    pub fn decode(d: &mut Data) -> Res<TransportParameters> {
        let mut tps = TransportParameters::default();

        while d.remaining() > 0 {
            match TransportParameter::decode(d) {
                Ok((tipe, tp)) => {
                    tps.params.insert(tipe, tp);
                }
                Err(Error::UnknownTransportParameter) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(tps)
    }

    // Get an integer type or a default.
    pub fn get_integer(&self, tipe: u16) -> u64 {
        let default = match tipe {
            TRANSPORT_PARAMETER_IDLE_TIMEOUT
            | TRANSPORT_PARAMETER_INITIAL_MAX_DATA
            | TRANSPORT_PARAMETER_INITIAL_MAX_STREAM_DATA_BIDI_LOCAL
            | TRANSPORT_PARAMETER_INITIAL_MAX_STREAM_DATA_BIDI_REMOTE
            | TRANSPORT_PARAMETER_INITIAL_MAX_STREAM_DATA_UNI
            | TRANSPORT_PARAMETER_INITIAL_MAX_STREAMS_BIDI
            | TRANSPORT_PARAMETER_INITIAL_MAX_STREAMS_UNI => 0,
            TRANSPORT_PARAMETER_MAX_PACKET_SIZE => 65527,
            TRANSPORT_PARAMETER_ACK_DELAY_EXPONENT => 3,
            TRANSPORT_PARAMETER_MAX_ACK_DELAY => 25,
            _ => panic!("Transport parameter not known or not an Integer"),
        };
        match self.params.get(&tipe) {
            None => default,
            Some(TransportParameter::Integer(x)) => *x,
            _ => panic!("Internal error"),
        }
    }

    // Get an integer type or a default.
    pub fn set_integer(&mut self, tipe: u16, value: u64) {
        let default = match tipe {
            TRANSPORT_PARAMETER_IDLE_TIMEOUT
            | TRANSPORT_PARAMETER_INITIAL_MAX_DATA
            | TRANSPORT_PARAMETER_INITIAL_MAX_STREAM_DATA_BIDI_LOCAL
            | TRANSPORT_PARAMETER_INITIAL_MAX_STREAM_DATA_BIDI_REMOTE
            | TRANSPORT_PARAMETER_INITIAL_MAX_STREAM_DATA_UNI
            | TRANSPORT_PARAMETER_INITIAL_MAX_STREAMS_BIDI
            | TRANSPORT_PARAMETER_INITIAL_MAX_STREAMS_UNI
            | TRANSPORT_PARAMETER_MAX_PACKET_SIZE
            | TRANSPORT_PARAMETER_ACK_DELAY_EXPONENT
            | TRANSPORT_PARAMETER_MAX_ACK_DELAY => {
                self.params.insert(tipe, TransportParameter::Integer(value))
            }
            _ => panic!("Transport parameter not known"),
        };
    }

    pub fn get_bytes(&self, tipe: u16) -> Option<Vec<u8>> {
        match tipe {
            TRANSPORT_PARAMETER_ORIGINAL_CONNECTION_ID
            | TRANSPORT_PARAMETER_STATELESS_RESET_TOKEN => {}
            _ => panic!("Transport parameter not known or not type bytes"),
        }

        match self.params.get(&tipe) {
            None => None,
            Some(TransportParameter::Bytes(x)) => Some(x.to_vec()),
            _ => panic!("Internal error"),
        }
    }

    pub fn set_bytes(&mut self, tipe: u16, value: Vec<u8>) {
        match tipe {
            TRANSPORT_PARAMETER_ORIGINAL_CONNECTION_ID
            | TRANSPORT_PARAMETER_STATELESS_RESET_TOKEN => {
                self.params.insert(tipe, TransportParameter::Bytes(value));
            }
            _ => panic!("Transport parameter not known or not type bytes"),
        }
    }

    fn was_sent(&self, tipe: u16) -> bool {
        self.params.contains_key(&tipe)
    }
}

#[derive(Default, Debug)]
pub struct TransportParametersHandler {
    pub local: TransportParameters,
    pub remote: Option<TransportParameters>,
}

impl ExtensionHandler for TransportParametersHandler {
    fn write(&mut self, msg: HandshakeMessage, d: &mut [u8]) -> ExtensionWriterResult {
        if !matches!(msg, TLS_HS_CLIENT_HELLO | TLS_HS_ENCRYPTED_EXTENSIONS) {
            return ExtensionWriterResult::Skip;
        }

        log!(
            LogLevel::Debug,
            "Writing transport parameters, msg={:?}",
            msg
        );

        // TODO(ekr@rtfm.com): Modify to avoid a copy.
        let mut buf = Data::default();
        self.local
            .encode(&mut buf)
            .expect("Failed to encode transport parameters");
        assert!(buf.remaining() <= d.len());
        d[..buf.remaining()].copy_from_slice(&buf.as_mut_vec());
        ExtensionWriterResult::Write(buf.remaining())
    }

    fn handle(&mut self, msg: HandshakeMessage, d: &[u8]) -> ExtensionHandlerResult {
        log!(
            LogLevel::Debug,
            "Handling transport parameters, msg={:?} len={}",
            msg,
            d.len()
        );
        if !matches!(msg, TLS_HS_CLIENT_HELLO | TLS_HS_ENCRYPTED_EXTENSIONS) {
            return ExtensionHandlerResult::Alert(110); // unsupported_extension
        }

        // TODO(ekr@rtfm.com): Unnecessary copy.
        let mut buf = Data::from_slice(d);

        match TransportParameters::decode(&mut buf) {
            Err(_) => ExtensionHandlerResult::Alert(47), // illegal_parameter
            Ok(tp) => {
                self.remote = Some(tp);
                ExtensionHandlerResult::Ok
            }
        }
    }
}

// TODO(ekr@rtfm.com): Need to write more TP unit tests.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_tps() {
        let mut tps = TransportParameters::default();
        tps.params.insert(
            TRANSPORT_PARAMETER_STATELESS_RESET_TOKEN,
            TransportParameter::Bytes(vec![1, 2, 3, 4, 5, 6, 7, 8, 1, 2, 3, 4, 5, 6, 7, 8]),
        );
        tps.params.insert(
            TRANSPORT_PARAMETER_INITIAL_MAX_STREAMS_BIDI,
            TransportParameter::Integer(10),
        );

        let mut d = Data::default();
        tps.encode(&mut d).expect("Couldn't encode");

        let tps2 = TransportParameters::decode(&mut d).expect("Couldn't decode");
        assert_eq!(tps, tps2);

        println!("TPS = {:?}", tps);
        assert_eq!(tps2.get_integer(TRANSPORT_PARAMETER_IDLE_TIMEOUT), 0); // Default
        assert_eq!(tps2.get_integer(TRANSPORT_PARAMETER_MAX_ACK_DELAY), 25); // Default
        assert_eq!(
            tps2.get_integer(TRANSPORT_PARAMETER_INITIAL_MAX_STREAMS_BIDI),
            10
        ); // Sent
        assert_eq!(
            tps2.get_bytes(TRANSPORT_PARAMETER_STATELESS_RESET_TOKEN),
            Some(vec![1, 2, 3, 4, 5, 6, 7, 8, 1, 2, 3, 4, 5, 6, 7, 8])
        );
        assert_eq!(
            tps2.get_bytes(TRANSPORT_PARAMETER_ORIGINAL_CONNECTION_ID),
            None
        );
        assert_eq!(
            tps2.was_sent(TRANSPORT_PARAMETER_ORIGINAL_CONNECTION_ID),
            false
        );
        assert_eq!(
            tps2.was_sent(TRANSPORT_PARAMETER_STATELESS_RESET_TOKEN),
            true
        );
    }

}
