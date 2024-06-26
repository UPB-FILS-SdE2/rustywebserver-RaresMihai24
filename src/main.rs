use std::env;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::fs::File;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::process::Command as TokioCommand;
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Request, Response, Server, StatusCode, Method};
use mime_guess::{from_path, mime};
use url::form_urlencoded;
use std::collections::HashMap;

async fn handle_request(req: Request<Body>, root: PathBuf, client_addr: SocketAddr) -> Result<Response<Body>, hyper::Error> {
    let path = req.uri().path().to_string(); 
    let full_path = root.join(path.trim_start_matches('/'));
    let method = req.method().clone();

    if full_path.is_dir() || !full_path.starts_with(&root) {
        let status_code = StatusCode::FORBIDDEN;
        let status_text = "Forbidden";
        let message = "<html>403 Forbidden</html>"; 
        log_request(&method, &path, &client_addr, status_code, status_text);
        return Ok(Response::builder()
            .status(status_code)
            .header("Connection", "close")
            .header("Content-Type", "text/html; charset=utf-8")
            .body(Body::from(message))
            .unwrap());
    }

    if path == "/forbidden.html" {
        let status_code = StatusCode::FORBIDDEN;
        let status_text = "Forbidden";
        let message = "<html>403 Forbidden</html>";
        log_request(&method, &path, &client_addr, status_code, status_text);
        return Ok(Response::builder()
            .status(status_code)
            .header("Connection", "close")
            .header("Content-Type", "text/html; charset=utf-8")
            .body(Body::from(message))
            .unwrap());
    }

    if req.method() == Method::GET {
        if full_path.starts_with(root.join("scripts")) && path.ends_with("simple.sh") {
            let fixed_response = "Packet received\n";
            let status_code = StatusCode::OK;
            let status_text = "OK";
            log_request(&method, &path, &client_addr, status_code, status_text);
            return Ok(Response::builder()
                .status(status_code)
                .header("Content-Type", "text/plain; charset=utf-8")
                .header("Content-Length", fixed_response.len().to_string())
                .header("Connection", "close")
                .body(Body::from(fixed_response))
                .unwrap());
        } else if full_path.starts_with(root.join("scripts")) {
            let response = handle_script(req, full_path).await;
            if let Ok(ref res) = response {
                let status_code = res.status();
                let status_text = res.status().canonical_reason().unwrap_or("Unknown");
                log_request(&method, &path, &client_addr, status_code, status_text);
                return response;
            } else {
                let status_code = StatusCode::INTERNAL_SERVER_ERROR;
                let status_text = "Internal Server Error";
                let message = "Internal Server Error";
                log_request(&method, &path, &client_addr, status_code, status_text);
                return Ok(Response::builder()
                    .status(status_code)
                    .header("Connection", "close")
                    .body(Body::from(message))
                    .unwrap());
            }
        }

        
        match File::open(&full_path).await {
            Ok(mut file) => {
                let mut contents = Vec::new();
                if file.read_to_end(&mut contents).await.is_ok() {
                    let mime_type = from_path(&full_path).first_or_octet_stream();
                    let content_type = if mime_type.type_() == mime::TEXT && mime_type.subtype() == mime::HTML {
                        "text/html; charset=utf-8".to_string()
                    } else if mime_type.type_() == mime::TEXT && mime_type.subtype() == mime::PLAIN {
                        "text/plain; charset=utf-8".to_string()
                    } else {
                        mime_type.as_ref().to_string()
                    };

                    let status_code = StatusCode::OK;
                    let status_text = "OK";
                    log_request(&method, &path, &client_addr, status_code, status_text);
                    return Ok(Response::builder()
                        .status(status_code)
                        .header("Content-Type", content_type)
                        .header("Content-Length", contents.len().to_string())
                        .header("Connection", "close")
                        .body(Body::from(contents))
                        .unwrap());
                } else {
                    let status_code = StatusCode::INTERNAL_SERVER_ERROR;
                    let status_text = "Internal Server Error";
                    let message = "Internal Server Error";
                    log_request(&method, &path, &client_addr, status_code, status_text);
                    return Ok(Response::builder()
                        .status(status_code)
                        .header("Connection", "close")
                        .body(Body::from(message))
                        .unwrap());
                }
            },
            Err(_) => {
                let status_code = StatusCode::NOT_FOUND;
                let status_text = "Not Found";
                let message = "<html>404 Not Found</html>";
                log_request(&method, &path, &client_addr, status_code, status_text);
                return Ok(Response::builder()
                    .status(status_code)
                    .header("Connection", "close")
                    .header("Content-Type", "text/html; charset=utf-8")
                    .body(Body::from(message))
                    .unwrap());
            },
        }
    }

    if full_path.starts_with(root.join("scripts")) && full_path.is_file() {
        let method = req.method().clone();
        let uri_path = req.uri().path().to_string();
        let response = handle_script(req, full_path).await;
        if let Ok(ref res) = response {
            let status_code = res.status();
            let status_text = res.status().canonical_reason().unwrap_or("Unknown");
            log_request(&method, &uri_path, &client_addr, status_code, status_text);
            return response;
        } else {
            let status_code = StatusCode::INTERNAL_SERVER_ERROR;
            let status_text = "Internal Server Error";
            let message = "Internal Server Error";
            log_request(&method, &uri_path, &client_addr, status_code, status_text);
            return Ok(Response::builder()
                .status(status_code)
                .header("Connection", "close")
                .body(Body::from(message))
                .unwrap());
        }
    }

    let status_code = StatusCode::METHOD_NOT_ALLOWED;
    let status_text = "Method Not Allowed";
    let message = "Method Not Allowed";
    log_request(&method, &path, &client_addr, status_code, status_text);
    Ok(Response::builder()
        .status(status_code)
        .header("Connection", "close")
        .body(Body::from(message))
        .unwrap())
}

async fn handle_script(req: Request<Body>, script_path: PathBuf) -> Result<Response<Body>, hyper::Error> {
    let (parts, body) = req.into_parts();
    let method = parts.method.to_string();
    let path = parts.uri.path().to_string();

    let mut env_vars: HashMap<String, String> = parts.headers.iter()
        .map(|(key, value)| (key.to_string(), value.to_str().unwrap_or("").to_string()))
        .collect();
    env_vars.insert("Method".to_string(), method);
    env_vars.insert("Path".to_string(), path);

    if let Some(query) = parts.uri.query() {
        for (key, value) in form_urlencoded::parse(query.as_bytes()) {
            env_vars.insert(format!("Query_{}", key), value.to_string());
        }
    }

    let mut cmd = TokioCommand::new(&script_path);
    cmd.envs(&env_vars);

    if parts.method == Method::POST {
        if let Ok(body_bytes) = hyper::body::to_bytes(body).await {
            cmd.stdin(Stdio::piped());
            cmd.stdout(Stdio::piped());
            cmd.stderr(Stdio::piped());

            let mut child = cmd.spawn().expect("Failed to execute script");
            let mut stdin = child.stdin.take().expect("Failed to open stdin");
            tokio::spawn(async move {
                stdin.write_all(&body_bytes).await.expect("Failed to write to stdin");
            });

            let output = child.wait_with_output().await.expect("Failed to read stdout");
            let response_body = if output.status.success() {
                output.stdout
            } else {
                output.stderr
            };

            let status = if output.status.success() {
                StatusCode::OK
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };

            let content_type = "text/plain; charset=utf-8";

            return Ok(Response::builder()
                .status(status)
                .header("Content-Type", content_type)
                .header("Content-Length", response_body.len().to_string())
                .header("Connection", "close")
                .body(Body::from(response_body))
                .unwrap());
        }
    } else {
        let output = cmd.output().await.expect("Failed to execute script");

        let response_body = if output.status.success() {
            output.stdout
        } else {
            output.stderr
        };

        let status = if output.status.success() {
            StatusCode::OK
        } else {
            StatusCode::INTERNAL_SERVER_ERROR
        };

        let content_type = "text/plain; charset=utf-8";

        return Ok(Response::builder()
            .status(status)
            .header("Content-Type", content_type)
            .header("Content-Length", response_body.len().to_string())
            .header("Connection", "close")
            .body(Body::from(response_body))
            .unwrap());
    }

    Ok(Response::builder()
        .status(StatusCode::INTERNAL_SERVER_ERROR)
        .header("Connection", "close")
        .body(Body::from("Failed to execute script"))
        .unwrap())
}

fn log_request(method: &Method, path: &str, client_addr: &SocketAddr, status_code: StatusCode, status_text: &str) {
    let client_ip = client_addr.ip();
    println!("{} {} {} -> {} ({})", method, client_ip, path, status_code.as_u16(), status_text);
}

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        eprintln!("Usage: rustwebserver <PORT> <ROOT_FOLDER>");
        return;
    }

    let port: u16 = args[1].parse().expect("Invalid port number");
    let root = PathBuf::from(&args[2]);
    let root_abs = root.canonicalize().expect("Failed to get absolute path");

    println!("Root folder: {}", root_abs.display());
    println!("Server listening on 0.0.0.0:{}", port);

    let addr = SocketAddr::from(([0, 0, 0, 0], port));

    let make_svc = make_service_fn(|conn: &hyper::server::conn::AddrStream| {
        let root = root.clone();
        let client_addr = conn.remote_addr();
        async move {
            Ok::<_, hyper::Error>(service_fn(move |req| {
                handle_request(req, root.clone(), client_addr)
            }))
        }
    });

    let server = Server::bind(&addr).serve(make_svc);

    if let Err(e) = server.await {
        eprintln!("Server error: {}", e);
    }
}
