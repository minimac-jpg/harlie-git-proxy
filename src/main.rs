mod api;
mod audit;
mod config;
mod scm;
mod state;
mod token;

use std::sync::Arc;

use hyper_util::rt::TokioExecutor;
use hyper_util::rt::TokioIo;
use hyper_util::service::TowerToHyperService;
use tokio::net::TcpListener;
use tokio_rustls::TlsAcceptor;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .json()
        .init();
    let config = config::Config::load()?;
    let bind = config.bind.clone();
    let tls_cert = config.tls_cert_path.clone();
    let tls_key = config.tls_key_path.clone();
    let state = state::AppState::new(config).await?;
    let app = api::router(state);
    let listener = TcpListener::bind(&bind).await?;
    tracing::info!(%bind, tls = tls_cert.is_some(), "harlie-git-proxy listening");

    if let (Some(cert_path), Some(key_path)) = (tls_cert, tls_key) {
        let acceptor = Arc::new(tls_acceptor(&cert_path, &key_path)?);
        loop {
            let (stream, _) = listener.accept().await?;
            let acceptor = acceptor.clone();
            let app = app.clone();
            tokio::spawn(async move {
                let tls = match acceptor.accept(stream).await {
                    Ok(tls) => tls,
                    Err(error) => {
                        tracing::debug!(error = %error, "tls handshake failed");
                        return;
                    }
                };
                let service = TowerToHyperService::new(app);
                if let Err(error) =
                    hyper_util::server::conn::auto::Builder::new(TokioExecutor::new())
                        .serve_connection_with_upgrades(TokioIo::new(tls), service)
                        .await
                {
                    tracing::debug!(error = %error, "tls connection ended");
                }
            });
        }
    } else {
        axum::serve(listener, app).await?;
    }
    Ok(())
}

fn tls_acceptor(
    cert_path: &str,
    key_path: &str,
) -> Result<TlsAcceptor, Box<dyn std::error::Error>> {
    let certs = rustls_pemfile::certs(&mut std::io::BufReader::new(std::fs::File::open(
        cert_path,
    )?))
    .collect::<Result<Vec<_>, _>>()?;
    let key =
        rustls_pemfile::private_key(&mut std::io::BufReader::new(std::fs::File::open(key_path)?))?
            .ok_or("TLS key file does not contain a private key")?;
    let config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)?;
    Ok(TlsAcceptor::from(Arc::new(config)))
}
