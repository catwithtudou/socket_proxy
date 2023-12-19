use log::debug;
use std::ops::Range;
use std::str::from_utf8;

const EXT_SERVER_NAME: &[u8] = &[0, 0];

// slice_by_at_range 获取 len_range 之内的数据
fn slice_by_at_range(data: &[u8], len_range: Range<usize>) -> Result<&[u8], &'static str> {
    let len_in_bits = data
        .get(len_range.clone())
        .ok_or("no enough data length to decode")?;
    let mut actual_len = 0usize;
    for &bit in len_in_bits {
        actual_len = actual_len << 8 | (bit as usize);
    }
    data.get(len_range.end..len_range.end + actual_len)
        .ok_or("error when get index")
}

// truncate_before 移除 len_range.end 之前的数据，保留其后的数据
fn truncate_before(data: &[u8], len_range: Range<usize>) -> Result<&[u8], &'static str> {
    let len = slice_by_at_range(data, len_range.clone())?.len();
    Ok(&data[len_range.end + len..])
}

pub struct TlsRecord<'a> {
    content_type: u8,
    major_version: u8,
    minor_version: u8,
    fragment: &'a [u8],
}

// 目前仅关心 server_name
pub struct TlsClientHello {
    pub server_name: Option<Box<str>>,
}

pub fn parse_tls_record<'a>(data: &'a [u8]) -> Result<TlsRecord<'a>, &'static str> {
    let fragment = slice_by_at_range(&data, 3..5)?;
    Ok(TlsRecord {
        content_type: data[0],
        major_version: data[1],
        minor_version: data[2],
        fragment,
    })
}

pub fn parse_client_hello(data: &[u8]) -> Result<TlsClientHello, &'static str> {
    let TlsRecord {
        content_type,
        major_version,
        minor_version,
        fragment,
    } = parse_tls_record(&data)?;
    if major_version != 3 {
        return Err("unknown tls version");
    }
    if content_type != 22 {
        return Err("not a handshake");
    }
    if fragment.get(0) != Some(&1) {
        return Err("handshake type isn't a client hello");
    }

    // Handshake Protocol Client Hello Length is 3 bytes
    let client_hello_body = slice_by_at_range(&fragment, 1..4)?;
    // version: TLS 1.2 (0x0303)
    if client_hello_body.get(0) != Some(&0x03) {
        return Err("unsupported TLS version");
    }
    // Random 32bytes
    // Session ID Length 2 bytes
    // Session ID
    // 34..35 Session ID Length
    let remaining = truncate_before(&client_hello_body, 34..35)?;
    // Cipher Suites Length
    let remaining = truncate_before(&remaining, 0..2)?;
    // compression method
    let remaining = truncate_before(&remaining, 0..1)?;
    // extensions length
    let mut exts = slice_by_at_range(&remaining, 0..2)?;
    // extensions
    // type 2 bytes
    // length 2 bytes
    let mut server_name = None;
    while exts.len() > 4 {
        let ext_type = &exts[0..2];
        let ext_data = slice_by_at_range(&exts, 2..4)?;
        // 移除掉当前extension
        // 这样 exts 就以下一次extension开头
        exts = truncate_before(&ext_data, 2..4)?;
        if ext_type == EXT_SERVER_NAME {
            // server_name extension
            if ext_data[3] == 0x00 {
                let raw_name = slice_by_at_range(&ext_data, 3..5)?;
                let raw_name =
                    from_utf8(&raw_name).map_err(|_| "error when parse from raw data")?;
                server_name = Some(String::from(raw_name).into_boxed_str());
                debug!("TLS parser domain: {}", server_name.as_ref().unwrap());
            }
        }
    }

    Ok(TlsClientHello { server_name })
}
