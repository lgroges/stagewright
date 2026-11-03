//! `stagewright serve`: the editor in a browser, backed by the same API the
//! desktop app uses. Binds to 127.0.0.1 only.

use stagewright_api::{ApiError, Workspace, dispatch};
use std::path::{Component, Path, PathBuf};
use tiny_http::{Header, Method, Response, Server};

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|e| e.to_str()).unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "ico" => "image/x-icon",
        "woff2" => "font/woff2",
        _ => "application/octet-stream",
    }
}

fn header(k: &str, v: &str) -> Header {
    Header::from_bytes(k.as_bytes(), v.as_bytes()).expect("static header")
}

/// Resolve a URL path inside `dir`, refusing anything that climbs out.
fn static_path(dir: &Path, url_path: &str) -> Option<PathBuf> {
    let rel = url_path.trim_start_matches('/');
    let rel = if rel.is_empty() { "index.html" } else { rel };
    let rel = Path::new(rel);
    if rel.components().any(|c| !matches!(c, Component::Normal(_))) {
        return None;
    }
    let p = dir.join(rel);
    if p.is_file() {
        Some(p)
    } else {
        Some(dir.join("index.html"))
    }
}

fn header_value<'a>(req: &'a tiny_http::Request, name: &'static str) -> Option<&'a str> {
    req.headers()
        .iter()
        .find(|h| h.field.equiv(name))
        .map(|h| h.value.as_str())
}

/// Only the editor itself may call the API. Requiring a JSON content type forces
/// browsers to preflight cross-site requests (which we never approve); checking
/// Host defeats DNS rebinding; checking Origin (when sent) rejects other sites.
fn same_origin(req: &tiny_http::Request, port: u16) -> Result<(), &'static str> {
    let allowed_hosts = [format!("127.0.0.1:{port}"), format!("localhost:{port}")];
    let host = header_value(req, "Host").unwrap_or("");
    if !allowed_hosts.iter().any(|h| h == host) {
        return Err("bad host");
    }
    if let Some(origin) = header_value(req, "Origin") {
        let ok = allowed_hosts.iter().any(|h| origin == format!("http://{h}"));
        if !ok {
            return Err("cross-origin request refused");
        }
    }
    let ct = header_value(req, "Content-Type").unwrap_or("");
    if !ct.starts_with("application/json") {
        return Err("Content-Type must be application/json");
    }
    Ok(())
}

pub fn run(root: Option<PathBuf>, port: u16, static_dir: Option<PathBuf>) -> Result<(), String> {
    let server = Server::http(("127.0.0.1", port)).map_err(|e| e.to_string())?;
    if let Some(r) = &root {
        stagewright_api::warm_up(r);
    }
    let mut ws = Workspace::new(root.clone());
    if let Some(r) = &root {
        dispatch(&mut ws, "project_open", serde_json::json!({ "root": r })).map_err(|e| e.to_string())?;
    }
    eprintln!("stagewright: http://127.0.0.1:{port}/");
    if let Some(r) = &root {
        eprintln!("project: {}", r.display());
    }
    match &static_dir {
        Some(d) => eprintln!("serving editor from {}", d.display()),
        None => eprintln!("API only (no --static); run the Vite dev server for the editor"),
    }

    for mut req in server.incoming_requests() {
        let url = req.url().split('?').next().unwrap_or("/").to_string();
        let resp = if let Some(cmd) = url.strip_prefix("/api/") {
            if *req.method() != Method::Post {
                Response::from_string("POST only").with_status_code(405).boxed()
            } else if let Err(why) = same_origin(&req, port) {
                Response::from_string(why).with_status_code(403).boxed()
            } else {
                let mut body = String::new();
                let _ = req.as_reader().read_to_string(&mut body);
                let args = if body.trim().is_empty() {
                    Ok(serde_json::Value::Null)
                } else {
                    serde_json::from_str(&body).map_err(|e| ApiError::new("bad_json", e.to_string()))
                };
                let (status, out) = match args.and_then(|a| dispatch(&mut ws, cmd, a)) {
                    Ok(v) => (200, v),
                    Err(e) => (400, serde_json::to_value(&e).unwrap()),
                };
                Response::from_string(out.to_string())
                    .with_status_code(status)
                    .with_header(header("Content-Type", "application/json"))
                    .boxed()
            }
        } else if let Some(dir) = &static_dir {
            match static_path(dir, &url).and_then(|p| std::fs::read(&p).ok().map(|b| (p, b))) {
                Some((p, bytes)) => Response::from_data(bytes)
                    .with_header(header("Content-Type", content_type(&p)))
                    .with_header(header("Cache-Control", "no-store"))
                    .boxed(),
                None => Response::from_string("not found").with_status_code(404).boxed(),
            }
        } else {
            Response::from_string("not found").with_status_code(404).boxed()
        };
        let _ = req.respond(resp);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_paths_cannot_escape() {
        let d = Path::new("/srv/dist");
        assert!(static_path(d, "/../etc/passwd").is_none());
        assert!(static_path(d, "/assets/../../x").is_none());
        assert_eq!(static_path(d, "/"), Some(d.join("index.html")));
    }
}
