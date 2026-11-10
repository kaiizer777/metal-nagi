use lambda_http::{run, service_fn, Body, Error, Request, Response};
use serde_json::json;
use tracing::info;

async fn handler(event: Request) -> Result<Response<Body>, Error> {
    info!(
        method = %event.method(),
        uri = %event.uri(),
        "processing lambda request"
    );

    let payload = status_payload();
    let resp = Response::builder()
        .status(200)
        .header("content-type", "application/json")
        .body(Body::Text(payload))
        .map_err(Box::new)?;

    Ok(resp)
}

fn status_payload() -> String {
    json!({
        "status": "ok",
        "bot": "nagi",
        "version": env!("CARGO_PKG_VERSION")
    })
    .to_string()
}

#[tokio::main]
async fn main() -> Result<(), Error> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .without_time()
        .init();

    run(service_fn(handler)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_status_payload_fields() {
        let payload = status_payload();
        let val: serde_json::Value = serde_json::from_str(&payload).expect("valid json");
        assert_eq!(val["status"], "ok");
        assert_eq!(val["bot"], "nagi");
        assert_eq!(val["version"], env!("CARGO_PKG_VERSION"));
    }

    #[tokio::test]
    async fn test_handler_returns_200_json() {
        let req = Request::default();
        let resp = handler(req).await.expect("handler success");
        assert_eq!(resp.status(), 200);
        assert_eq!(
            resp.headers().get("content-type").unwrap(),
            "application/json"
        );
    }
}
