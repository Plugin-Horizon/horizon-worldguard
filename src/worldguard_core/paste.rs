//! The paste upload of `/wg report -p` and `/wg debug ... -s` (`EngineHubPaste`): ask the service for a signed upload
//! (`signed_paste_v2`, the `x-paste-meta-*` headers), PUT the text there with the headers it names, and return the view
//! URL. 200 and 204 are success for the upload.

pub const SERVICE: &str = "https://paste.enginehub.org";

/// What the paste is called: `Event debugging report: %s.txt` becomes the extension and name.
pub struct Meta<'a> {
    pub name: Option<&'a str>,
    pub author: Option<&'a str>,
    pub extension: Option<&'a str>,
}

/// Uploads `content` to the service at `base` and returns the view URL.
pub fn upload(base: &str, content: &str, meta: &Meta<'_>) -> Result<String, String> {
    let agent = ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(15)).build();
    let mut sign = agent.get(&format!("{}/signed_paste_v2", base.trim_end_matches('/'))).set("x-paste-meta-from", "EngineHub");
    for (k, v) in [("name", meta.name), ("author", meta.author), ("extension", meta.extension)] {
        if let Some(v) = v {
            sign = sign.set(&format!("x-paste-meta-{k}"), v);
        }
    }
    let resp = match sign.call() {
        Ok(r) => r,
        Err(ureq::Error::Status(code, r)) => {
            return Err(format!("Failed to initiate paste upload: {code} {}", r.into_string().unwrap_or_default()));
        }
        Err(e) => return Err(format!("Failed to initiate paste upload: {e}")),
    };
    let body = resp.into_string().map_err(|e| e.to_string())?;
    let json: serde_json::Value = serde_json::from_str(&body).map_err(|e| format!("Bad paste response: {e}"))?;
    let field = |k: &str| json.get(k).and_then(|v| v.as_str()).map(str::to_owned).ok_or_else(|| format!("Bad paste response: no {k}"));
    let (view, up) = (field("viewUrl")?, field("uploadUrl")?);
    let mut put = agent.put(&up);
    if let Some(h) = json.get("headers").and_then(|h| h.as_object()) {
        for (k, v) in h {
            if let Some(v) = v.as_str() {
                put = put.set(k, v);
            }
        }
    }
    match put.send_string(content) {
        Ok(r) if r.status() == 200 || r.status() == 204 => Ok(view),
        Ok(r) => Err(format!("Failed to upload paste: {}", r.status())),
        Err(ureq::Error::Status(code, r)) => Err(format!("Failed to upload paste: {code} {}", r.into_string().unwrap_or_default())),
        Err(e) => Err(format!("Failed to upload paste: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// A server that answers the sign request with JSON naming itself as the upload target, then takes the PUT.
    #[test]
    fn signs_then_puts_and_returns_the_view_url() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let t = std::thread::spawn(move || {
            let mut seen = Vec::new();
            for i in 0..2 {
                let (mut s, _) = l.accept().unwrap();
                let mut buf = [0u8; 4096];
                let n = s.read(&mut buf).unwrap();
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                seen.push(req);
                let (status, body) = if i == 0 {
                    (
                        "200 OK",
                        format!("{{\"viewUrl\":\"https://v/x\",\"uploadUrl\":\"http://127.0.0.1:{port}/up\",\"headers\":{{\"x-test\":\"1\"}}}}"),
                    )
                } else {
                    ("204 No Content", String::new())
                };
                write!(s, "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
            }
            seen
        });
        let url = upload(&format!("http://127.0.0.1:{port}"), "hello", &Meta { name: Some("n"), author: None, extension: Some("txt") }).unwrap();
        assert_eq!(url, "https://v/x");
        let seen = t.join().unwrap();
        let low = seen[0].to_ascii_lowercase();
        assert!(seen[0].starts_with("GET /signed_paste_v2") && low.contains("x-paste-meta-from: enginehub") && low.contains("x-paste-meta-extension: txt"));
        assert!(seen[1].starts_with("PUT /up") && seen[1].to_ascii_lowercase().contains("x-test: 1") && seen[1].ends_with("hello"));
    }

    #[test]
    fn a_failed_sign_is_reported() {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (mut s, _) = l.accept().unwrap();
            let mut b = [0u8; 1024];
            let _ = s.read(&mut b);
            write!(s, "HTTP/1.1 500 Err\r\nContent-Length: 2\r\nConnection: close\r\n\r\nno").unwrap();
        });
        let e = upload(&format!("http://127.0.0.1:{port}"), "x", &Meta { name: None, author: None, extension: None }).unwrap_err();
        assert!(e.starts_with("Failed to initiate paste upload: 500"), "{e}");
    }
}
