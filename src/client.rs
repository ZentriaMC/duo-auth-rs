use std::{future::Future, sync::Arc, time::Duration};

use reqwest::{Method, StatusCode, Url};
use serde::Deserialize;

use super::{
    request::{DuoRequest, Parameters},
    types::PreauthResponse,
};
use crate::Error;

pub struct DuoClient(Arc<DuoClientInner>);

struct DuoClientInner {
    base_url: Url,
    ikey: String,
    skey: String,

    client: reqwest::Client,
}

#[allow(dead_code)]
#[derive(Deserialize)]
struct DuoResponse<T> {
    response: T,
    stat: String,

    code: Option<u64>,
    message: Option<String>,
}

impl DuoClient {
    pub fn new(api_domain: String, ikey: String, skey: String) -> Result<DuoClient, Error> {
        let base_url = Url::parse(&api_domain)?;

        // Fail fast when there's no domain
        let _ = base_url
            .host_str()
            .expect("No domain in provided api_domain") // TODO: error
            .to_string();

        let client = reqwest::Client::new();
        Ok(DuoClient(Arc::new(DuoClientInner {
            base_url,
            ikey,
            skey,
            client,
        })))
    }

    pub fn check(&self) -> impl Future<Output = Result<u64, Error>> {
        let this = Arc::clone(&self.0);

        async move {
            let request = DuoRequest::new(
                this.base_url.clone(),
                Method::GET,
                "/auth/v2/check",
                Parameters::default(),
            )
            .build(&this.client, &this.ikey, &this.skey)?;

            let response = this.client.execute(request).await?;
            if response.status() != StatusCode::OK {
                // TODO: handle error properly
                let status = response.status();
                let errbody: serde_json::Value = response.json().await?;
                println!("err body={:?}", errbody);
                return Err(Error::from(format!("status code={}", status)));
            }

            #[derive(Deserialize)]
            struct CheckResponse {
                time: u64,
            }

            let body: DuoResponse<CheckResponse> = response.json().await?;

            Ok(body.response.time)
        }
    }

    pub fn preauth<S: Into<String>>(
        &self,
        user_id: S,
    ) -> impl Future<Output = Result<PreauthResponse, Error>> {
        let this = Arc::clone(&self.0);

        async move { DuoClient::request_preauth(this, user_id).await }
    }

    pub fn auth<S: Into<String>>(
        &self,
        user_id: S,
        share_n: usize,
    ) -> impl Future<Output = Result<bool, Error>> {
        let this = Arc::clone(&self.0);

        async move {
            let txid = DuoClient::request_auth(this.clone(), user_id, share_n).await?;
            let mut status: Option<bool>;

            loop {
                status = DuoClient::request_auth_status(this.clone(), &txid).await?;
                match status {
                    None => tokio::time::sleep(Duration::from_secs(2)).await,
                    Some(v) => return Ok(v),
                }
            }
        }
    }

    async fn request_preauth<S: Into<String>>(
        this: Arc<DuoClientInner>,
        user_id: S,
    ) -> Result<PreauthResponse, Error> {
        let mut parameters = Parameters::default();
        parameters.set("user_id", user_id);

        let request = DuoRequest::new(
            this.base_url.clone(),
            Method::POST,
            "/auth/v2/preauth",
            parameters,
        )
        .build(&this.client, &this.ikey, &this.skey)?;

        let response = this.client.execute(request).await?;
        if response.status() != StatusCode::OK {
            // TODO: handle error properly
            let status = response.status();
            let errbody: serde_json::Value = response.json().await?;
            println!("err body={:?}", errbody);
            return Err(Error::from(format!("status code={}", status)));
        }

        let body: DuoResponse<PreauthResponse> = response.json().await?;
        Ok(body.response)
    }

    async fn request_auth<S: Into<String>>(
        this: Arc<DuoClientInner>,
        user_id: S,
        share_n: usize,
    ) -> Result<String, Error> {
        let mut parameters = Parameters::default();
        parameters.set("user_id", user_id);
        parameters.set("factor", "auto");
        parameters.set("async", "1");
        parameters.set("type", "Authorize share");
        parameters.set("device", "auto");
        parameters.set("display_username", format!("Share {}", share_n));

        let request = DuoRequest::new(
            this.base_url.clone(),
            Method::POST,
            "/auth/v2/auth",
            parameters,
        )
        .build(&this.client, &this.ikey, &this.skey)?;

        let response = this.client.execute(request).await?;
        if response.status() != StatusCode::OK {
            // TODO: handle error properly
            let status = response.status();
            let errbody: serde_json::Value = response.json().await?;
            println!("err body={:?}", errbody);
            return Err(Error::from(format!("status code={}", status)));
        }

        #[derive(Deserialize)]
        struct AuthResponse {
            txid: String,
        }

        let body: DuoResponse<AuthResponse> = response.json().await?;

        Ok(body.response.txid)
    }

    async fn request_auth_status(
        this: Arc<DuoClientInner>,
        txid: &str,
    ) -> Result<Option<bool>, Error> {
        let mut parameters = Parameters::default();
        parameters.set("txid", txid);

        let request = DuoRequest::new(
            this.base_url.clone(),
            Method::GET,
            "/auth/v2/auth_status",
            parameters,
        )
        .build(&this.client, &this.ikey, &this.skey)?;

        let response = this.client.execute(request).await?;
        if response.status() != StatusCode::OK {
            // TODO: handle error properly
            let status = response.status();
            let errbody: serde_json::Value = response.json().await?;
            println!("err body={:?}", errbody);
            return Err(Error::from(format!("status code={}", status)));
            //return Err(Error::from(format!("status code={}", response.status())));
        }

        #[derive(Deserialize)]
        struct AuthStatusResponse {
            result: String,
        }

        let body: DuoResponse<AuthStatusResponse> = response.json().await?;
        match body.response.result.as_str() {
            "waiting" => Ok(None),
            "allow" => Ok(Some(true)),
            "deny" => Ok(Some(false)),
            v => Err(Error::from(format!("unexpected result '{}'", v))),
        }
    }
}