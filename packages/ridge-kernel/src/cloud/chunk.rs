//! Existing browser SINGLE/CHUNK framing; one reassembler per ordered channel.
use std::collections::BTreeMap;
const WIRE:usize=16*1024;
const PART:usize=WIRE-9;
pub const MAX:usize=16*1024*1024+4096;
pub fn encode(bytes:&[u8],id:u32)->Result<Vec<Vec<u8>>,String> {
    if bytes.len()>MAX {return Err("cloud ciphertext too large".into());}
    if bytes.len()<WIRE {let mut wire=vec![0];wire.extend_from_slice(bytes);return Ok(vec![wire]);}
    let count=bytes.len().div_ceil(PART) as u16;
    Ok(bytes.chunks(PART).enumerate().map(|(index,part)| {
        let mut wire=vec![1];wire.extend_from_slice(&id.to_be_bytes());wire.extend_from_slice(&(index as u16).to_be_bytes());wire.extend_from_slice(&count.to_be_bytes());wire.extend_from_slice(part);wire
    }).collect())
}
#[derive(Default)]
pub struct Reassembler {id:Option<u32>,count:u16,total:usize,parts:BTreeMap<u16,Vec<u8>>}
impl Reassembler {
    pub fn push(&mut self,wire:&[u8])->Option<Vec<u8>> {
        if wire.is_empty() {return None;}
        if wire.len()>WIRE { *self=Self::default();return None;}
        if wire[0]==0 {*self=Self::default();return Some(wire[1..].to_vec());}
        if wire[0]!=1||wire.len()<9 {*self=Self::default();return None;}
        let id=u32::from_be_bytes(wire[1..5].try_into().ok()?);let index=u16::from_be_bytes(wire[5..7].try_into().ok()?);let count=u16::from_be_bytes(wire[7..9].try_into().ok()?);
        if count==0||index>=count {*self=Self::default();return None;}
        if self.id!=Some(id) {*self=Self {id:Some(id),count,..Default::default()};}
        if self.count!=count||self.parts.contains_key(&index)||self.total+wire.len()-9>MAX {*self=Self::default();return None;}
        self.total+=wire.len()-9;self.parts.insert(index,wire[9..].to_vec());
        if self.parts.len()!=count as usize {return None;}
        let mut bytes=Vec::with_capacity(self.total);for part in self.parts.values(){bytes.extend_from_slice(part);}*self=Self::default();Some(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn browser_wire_headers_and_large_frame_reassembly_are_compatible() {
        assert_eq!(encode(&[4,5],7).unwrap(),vec![vec![0,4,5]]);
        let bytes=(0..50000).map(|index|(index%251) as u8).collect::<Vec<_>>();
        let wire=encode(&bytes,0x01020304).unwrap();
        assert_eq!(&wire[0][..9],&[1,1,2,3,4,0,0,0,4]);assert!(wire.iter().all(|part|part.len()<=WIRE));
        let mut joined=Reassembler::default();let mut result=None;
        for part in wire.iter().rev(){if let Some(value)=joined.push(part){result=Some(value);}}
        assert_eq!(result,Some(bytes));
    }
    #[test]
    fn duplicate_and_oversized_parts_cannot_commit_partial_ciphertext() {
        let wire=encode(&vec![7;20000],1).unwrap();let mut joined=Reassembler::default();
        assert!(joined.push(&wire[0]).is_none());assert!(joined.push(&wire[0]).is_none());assert!(joined.push(&wire[1]).is_none());
        assert!(joined.push(&vec![0;WIRE+1]).is_none());assert_eq!(joined.push(&[0,8]),Some(vec![8]));
    }
}
