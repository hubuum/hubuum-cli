use hubuum_client::QueryObservationUpdateRequest;
use serde_json::{to_value, Value};

use crate::diagnostics::client_call;
use crate::errors::AppError;

use super::HubuumGateway;

impl HubuumGateway {
    pub fn query_observations(&self) -> Result<Value, AppError> {
        Ok(to_value(client_call("client.query_observations", || {
            self.client().query_observations()
        })?)?)
    }

    pub fn set_query_observations(&self, enabled: bool) -> Result<Value, AppError> {
        Ok(to_value(client_call(
            "client.set_query_observations",
            || {
                self.client()
                    .set_query_observations(QueryObservationUpdateRequest { enabled })
            },
        )?)?)
    }
    pub fn server_config(&self) -> Result<Value, AppError> {
        Ok(to_value(client_call("client.admin_config", || {
            self.client().admin_config()
        })?)?)
    }
}
