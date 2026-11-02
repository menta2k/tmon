//! DNS response builder (hickory-proto encoder).

use hickory_proto::op::{Message, MessageType, OpCode, Query, ResponseCode};
use hickory_proto::rr::rdata::CNAME;
use hickory_proto::rr::{Name, RData, Record, RecordType};
use std::net::IpAddr;

/// One answer: `owner` → address or CNAME target.
#[derive(Debug, Clone)]
pub enum Answer<'a> {
    Addr(&'a str, IpAddr, u32),
    Cname(&'a str, &'a str, u32),
}

fn name(s: &str) -> Name {
    Name::from_ascii(s).expect("fixture name")
}

/// Encoded response to `qname` (A, or AAAA when the first address is v6).
pub fn response(qname: &str, answers: &[Answer<'_>], rcode: ResponseCode) -> Vec<u8> {
    let mut m = Message::new(0x1234, MessageType::Response, OpCode::Query);
    m.metadata.response_code = rcode;
    m.add_query(Query::query(name(qname), RecordType::A));
    for a in answers {
        let rec = match a {
            Answer::Addr(owner, ip, ttl) => Record::from_rdata(name(owner), *ttl, RData::from(*ip)),
            Answer::Cname(owner, target, ttl) => {
                Record::from_rdata(name(owner), *ttl, RData::CNAME(CNAME(name(target))))
            }
        };
        m.add_answer(rec);
    }
    m.to_vec().expect("encode")
}

/// Encoded query (QR = 0).
pub fn query(qname: &str) -> Vec<u8> {
    let mut m = Message::new(7, MessageType::Query, OpCode::Query);
    m.add_query(Query::query(name(qname), RecordType::A));
    m.to_vec().expect("encode")
}
