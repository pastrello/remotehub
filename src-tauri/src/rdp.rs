//! RDP X.224 Connection Request/Confirm probe.
//! This stops before TLS/CredSSP and never transmits credentials.
//! Reference: MS-RDPBCGR connection sequence. Not an authenticated session.
use std::{io::{Read, Write}, net::{TcpStream, ToSocketAddrs}, time::Duration};

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NegotiationResult {
    pub address: String,
    pub security: String,
    pub note: String,
}
const REQUEST: [u8; 19] = [
    0x03,0x00,0x00,0x13, // TPKT v3, 19 bytes
    0x0e,0xe0,0x00,0x00,0x00,0x00,0x00, // X.224 CR
    0x01,0x00,0x08,0x00,0x0b,0x00,0x00,0x00, // RDP_NEG_REQ TLS, HYBRID, HYBRID_EX
];

fn parse_confirm(packet: &[u8]) -> Result<String, String> {
    if packet.len() < 11 || packet[0] != 3 || packet[1] != 0 {
        return Err("TPKT RDP inválido".into());
    }
    let length = u16::from_be_bytes([packet[2], packet[3]]) as usize;
    if length != packet.len() || length > 4096 {
        return Err("Comprimento TPKT inválido".into());
    }
    let x224_length = packet[4] as usize;
    if x224_length < 6 || x224_length + 5 != packet.len() {
        return Err(format!("Comprimento X.224 inesperado: LI={x224_length}, TPKT={length}"));
    }
    if packet[5] != 0xd0 {
        return Err("Resposta não é X.224 Connection Confirm".into());
    }
    // X.224 CC fixed portion: LI(1), type(1), DST-REF(2), SRC-REF(2), CLASS(1).
    // RDP_NEG_RSP or RDP_NEG_FAILURE follows at offset 11, not necessarily at
    // the last 8 bytes of an arbitrary packet.
    if packet.len() == 11 {
        return Ok("X.224 confirmado; protocolo de segurança não informado".into());
    }
    let data = &packet[11..];
    if data.len() != 8 {
        return Err(format!(
            "Dados de negociação RDP inesperados ({} bytes; início: {})",
            data.len(),
            hex_prefix(data, 16)
        ));
    }
    let flags = data[1];
    let field_length = u16::from_le_bytes([data[2], data[3]]);
    if field_length != 8 {
        return Err(format!(
            "RDP_NEG_RSP com comprimento inválido ({field_length}); bytes: {}",
            hex_prefix(data, 8)
        ));
    }
    let code = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    match data[0] {
        2 => match code {
            0 => Ok("RDP Standard (sem TLS/NLA selecionado)".into()),
            1 => Ok("TLS selecionado".into()),
            2 => Ok("CredSSP/NLA selecionado".into()),
            4 => Ok("CredSSP/NLA estendido selecionado".into()),
            8 => Ok("RDS AAD selecionado".into()),
            _ => Err(format!("Protocolo negociado desconhecido: {code} (flags={flags})")),
        },
        3 => Err(format!("Servidor recusou a negociação de segurança (código {code})")),
        kind => Err(format!("Tipo de negociação desconhecido: {kind}; bytes: {}",hex_prefix(data,8))),
    }
}
fn hex_prefix(bytes: &[u8], limit: usize) -> String {
    bytes.iter().take(limit).map(|b| format!("{b:02x}")).collect::<Vec<_>>().join(" ")
}

pub fn probe(host: &str, port: u16) -> Result<NegotiationResult,String> {
    if host.trim().is_empty() || host.chars().any(|c| c.is_control() || c.is_whitespace())
        || port == 0 {return Err("Host ou porta inválidos".into());}
    let address=if host.contains(':')&&!host.starts_with('['){format!("[{host}]:{port}")}else{format!("{host}:{port}")};
    let targets=address.to_socket_addrs().map_err(|e|format!("Falha DNS: {e}"))?;
    let mut last_error=String::from("Não há endereço de destino válido");
    for target in targets.take(4) {
        let mut stream=match TcpStream::connect_timeout(&target,Duration::from_secs(3)) {
            Ok(s)=>s,
            Err(e)=>{last_error=e.to_string();continue}
        };
        stream.set_read_timeout(Some(Duration::from_secs(4))).map_err(|e|e.to_string())?;
        stream.set_write_timeout(Some(Duration::from_secs(4))).map_err(|e|e.to_string())?;
        stream.write_all(&REQUEST).map_err(|e|format!("Falha enviando X.224: {e}"))?;
        let mut hdr=[0_u8;4];
        stream.read_exact(&mut hdr).map_err(|e|format!("Falha lendo TPKT: {e}"))?;
        let size=u16::from_be_bytes([hdr[2],hdr[3]]) as usize;
        if size < 11 || size > 4096 {return Err("Tamanho de resposta RDP inválido".into());}
        let mut p=vec![0_u8;size];
        p[..4].copy_from_slice(&hdr);
        stream.read_exact(&mut p[4..]).map_err(|e|format!("Resposta RDP truncada: {e}"))?;
        let security=parse_confirm(&p)?;
        return Ok(NegotiationResult {
            address,
            note:"X.224 negociado. TLS/NLA, login e desktop ainda não foram iniciados.".into(),
            security
        });
    }
    Err(format!("Não foi possível conectar via TCP: {last_error}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn valid_nla_response() {
        let packet=[3,0,0,19,14,0xd0,0,0,0,0,0,2,0,8,0,2,0,0,0];
        assert_eq!(parse_confirm(&packet).unwrap(),"CredSSP/NLA selecionado");
    }
    #[test]
    fn negotiation_failure_is_not_success() {
        let packet=[3,0,0,19,14,0xd0,0,0,0,0,0,3,0,8,0,2,0,0,0];
        assert!(parse_confirm(&packet).is_err());
    }
    #[test]
    fn selected_tls_is_accepted() {
        let packet=[3,0,0,19,14,0xd0,0,0,0,0,0,2,1,8,0,1,0,0,0];
        assert_eq!(parse_confirm(&packet).unwrap(),"TLS selecionado");
    }
    #[test]
    fn unexpected_negotiation_bytes_reported() {
        let packet=[3,0,0,19,14,0xd0,0,0,0,0,0,2,0,0,0,2,0,0,0];
        assert!(parse_confirm(&packet).unwrap_err().contains("02 00 00 00"));
    }
    #[test]
    fn malformed_packet_rejected() {assert!(parse_confirm(&[3,0,0,100,0]).is_err());}
}
