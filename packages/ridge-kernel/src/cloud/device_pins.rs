//! Native TOFU authority; preserve the browser's existing host keys and warn-only pins.
use serde::{Deserialize,Serialize};
use std::{collections::BTreeMap,path::Path,sync::Mutex,io::Read};
static WRITES:Mutex<()>=Mutex::new(());
const FILE:&str="cloud-device-fingerprints.json";
#[derive(Serialize,Deserialize)]
#[serde(deny_unknown_fields)]
struct Pins {version:u8,pins:BTreeMap<String,String>}
pub(crate) fn valid_fingerprint(value:&str)->bool {
    value.len()==19&&value.bytes().enumerate().all(|(index,byte)|if [4,9,14].contains(&index){byte==b'-'}else{byte.is_ascii_digit()||(b'A'..=b'F').contains(&byte)})
}
pub(crate) fn check(root:&Path,host:&str,actual:&str,legacy:Option<&str>)->Result<Option<String>,String>{
    if host.is_empty()||host.len()>201||!host.bytes().all(|byte|byte.is_ascii_alphanumeric()||matches!(byte,b'-'|b'_'))||!valid_fingerprint(actual)||legacy.is_some_and(|pin|!valid_fingerprint(pin)){return Err("invalid Cloud identity pin".into());}
    let _guard=WRITES.lock().map_err(|_|"Cloud identity pins unavailable")?;let path=root.join(FILE);
    let stored=std::fs::File::open(&path).and_then(|file|{let mut bytes=Vec::new();file.take(1024*1024+1).read_to_end(&mut bytes)?;Ok(bytes)});
    let mut value=match stored{
        Ok(bytes)=>{if bytes.len()>1024*1024{return Err("Cloud identity pins exceed bound; original preserved".into());}let value:Pins=serde_json::from_slice(&bytes).map_err(|_|"invalid Cloud identity pins; original preserved")?;
            if value.version!=1||value.pins.len()>4096||value.pins.iter().any(|(host,pin)|host.is_empty()||host.len()>201||!host.bytes().all(|byte|byte.is_ascii_alphanumeric()||matches!(byte,b'-'|b'_'))||!valid_fingerprint(pin)){return Err("invalid Cloud identity pins; original preserved".into());}value},
        Err(error) if error.kind()==std::io::ErrorKind::NotFound=>Pins{version:1,pins:BTreeMap::new()},
        Err(_)=>return Err("Cloud identity pins unreadable; original preserved".into()),
    };
    let pinned=if let Some(pin)=value.pins.get(host){pin.clone()}else{
        if value.pins.len()>=4096{return Err("Cloud identity pin count exceeds bound".into());}
        let pin=legacy.unwrap_or(actual).to_owned();value.pins.insert(host.into(),pin.clone());
        ridge_core::workspace::file::write_atomic(&path,&serde_json::to_vec_pretty(&value).map_err(|_|"Cloud identity serialization failed")?).map_err(|_|"Cloud identity pin persistence failed; original preserved")?;pin
    };
    Ok((pinned!=actual).then(||format!("设备指纹变化（原 {pinned} → 现 {actual}）")))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_pin_survives_identity_change_and_restart(){
        let root=std::env::temp_dir().join(format!("ridge-cloud-pins-{}",uuid::Uuid::new_v4()));std::fs::create_dir_all(&root).unwrap();
        let warning=check(&root,"desktop-owner","AAAA-BBBB-CCCC-DDDD",Some("1111-2222-3333-4444")).unwrap();assert!(warning.is_some());
        let bytes=std::fs::read(root.join(FILE)).unwrap();
        assert!(check(&root,"desktop-owner","AAAA-BBBB-CCCC-DDDD",Some("AAAA-BBBB-CCCC-DDDD")).unwrap().is_some());
        assert_eq!(std::fs::read(root.join(FILE)).unwrap(),bytes);
        assert!(check(&root,"desktop-owner","1111-2222-3333-4444",None).unwrap().is_none());
    }
    #[test]
    fn malformed_or_unwritable_store_keeps_original(){
        let root=std::env::temp_dir().join(format!("ridge-cloud-pins-{}",uuid::Uuid::new_v4()));std::fs::create_dir_all(&root).unwrap();let path=root.join(FILE);
        std::fs::write(&path,b"incomplete migration").unwrap();assert!(check(&root,"desktop-owner","AAAA-BBBB-CCCC-DDDD",None).is_err());assert_eq!(std::fs::read(&path).unwrap(),b"incomplete migration");
        let blocked=root.join("blocked");std::fs::write(&blocked,b"retain original").unwrap();assert!(check(&blocked,"desktop-owner","AAAA-BBBB-CCCC-DDDD",None).is_err());assert_eq!(std::fs::read(&blocked).unwrap(),b"retain original");
    }
}
