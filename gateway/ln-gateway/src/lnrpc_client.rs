use std::fmt::Debug;
use std::time::Duration;

use anyhow::anyhow;
use async_trait::async_trait;
use fedimint_core::task::sleep;
use futures::stream::BoxStream;
use tonic::transport::{Channel, Endpoint};
use tonic::Request;
use tracing::error;
use url::Url;

use crate::gatewaylnrpc::gateway_lightning_client::GatewayLightningClient;
use crate::gatewaylnrpc::{
    CompleteHtlcsRequest, CompleteHtlcsResponse, EmptyRequest, GetNodeInfoResponse,
    GetRouteHintsResponse, PayInvoiceRequest, PayInvoiceResponse, SubscribeInterceptHtlcsRequest,
    SubscribeInterceptHtlcsResponse,
};
use crate::{GatewayError, Result};

pub type HtlcStream<'a> =
    BoxStream<'a, std::result::Result<SubscribeInterceptHtlcsResponse, tonic::Status>>;

#[async_trait]
pub trait ILnRpcClient: Debug + Send + Sync {
    /// Get the public key and alias of the lightning node
    async fn info(&self) -> Result<GetNodeInfoResponse>;

    /// Get route hints to the lightning node
    async fn routehints(&self) -> Result<GetRouteHintsResponse>;

    /// Attempt to pay an invoice using the lightning node
    async fn pay(&self, invoice: PayInvoiceRequest) -> Result<PayInvoiceResponse>;

    /// Subscribe to intercept htlcs that belong to a specific mint identified
    /// by `short_channel_id`
    async fn subscribe_htlcs<'a>(
        &self,
        subscription: SubscribeInterceptHtlcsRequest,
    ) -> Result<HtlcStream<'a>>;

    /// Request completion of an intercepted htlc after processing and
    /// determining an outcome
    async fn complete_htlc(&self, outcome: CompleteHtlcsRequest) -> Result<CompleteHtlcsResponse>;
}

/// An `ILnRpcClient` that wraps around `GatewayLightningClient` for
/// convenience, and makes real RPC requests over the wire to a remote lightning
/// node. The lightning node is exposed via a corresponding
/// `GatewayLightningServer`.
#[derive(Debug)]
pub struct NetworkLnRpcClient {
    client: GatewayLightningClient<Channel>,
}

impl NetworkLnRpcClient {
    pub async fn new(url: Url) -> Result<Self> {
        let endpoint = Endpoint::from_shared(url.to_string()).map_err(|e| {
            error!("Failed to create lnrpc endpoint from url : {:?}", e);
            GatewayError::Other(anyhow!("Failed to create lnrpc endpoint from url"))
        })?;

        let gw_rpc = NetworkLnRpcClient {
            client: Self::connect(endpoint).await?,
        };
        Ok(gw_rpc)
    }

    async fn connect(endpoint: Endpoint) -> Result<GatewayLightningClient<Channel>> {
        let client = loop {
            match GatewayLightningClient::connect(endpoint.clone()).await {
                Ok(client) => break client,
                Err(_) => {
                    tracing::warn!("Couldn't connect to CLN extension, retrying in 5 seconds...");
                    sleep(Duration::from_secs(5)).await;
                }
            }
        };

        Ok(client)
    }
}

#[async_trait]
impl ILnRpcClient for NetworkLnRpcClient {
    async fn info(&self) -> Result<GetNodeInfoResponse> {
        let req = Request::new(EmptyRequest {});
        let mut client = self.client.clone();
        let res = client.get_node_info(req).await?;
        return Ok(res.into_inner());
    }

    async fn routehints(&self) -> Result<GetRouteHintsResponse> {
        let req = Request::new(EmptyRequest {});
        let mut client = self.client.clone();
        let res = client.get_route_hints(req).await?;
        return Ok(res.into_inner());
    }

    async fn pay(&self, invoice: PayInvoiceRequest) -> Result<PayInvoiceResponse> {
        let req = Request::new(invoice);
        let mut client = self.client.clone();
        let res = client.pay_invoice(req).await?;
        return Ok(res.into_inner());
    }

    async fn subscribe_htlcs<'a>(
        &self,
        subscription: SubscribeInterceptHtlcsRequest,
    ) -> Result<HtlcStream<'a>> {
        let req = Request::new(subscription);
        let mut client = self.client.clone();
        let res = client.subscribe_intercept_htlcs(req).await?;
        return Ok(Box::pin(res.into_inner()));
    }

    async fn complete_htlc(&self, outcome: CompleteHtlcsRequest) -> Result<CompleteHtlcsResponse> {
        let req = Request::new(outcome);
        let mut client = self.client.clone();
        let res = client.complete_htlc(req).await?;
        return Ok(res.into_inner());
    }
}
