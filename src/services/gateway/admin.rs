use serde_json::{to_value, Value};

use crate::diagnostics::client_call;
use crate::errors::AppError;

use super::HubuumGateway;

impl HubuumGateway {
    pub fn server_config(&self) -> Result<Value, AppError> {
        Ok(to_value(client_call("client.admin_config", || {
            self.client().admin_config()
        })?)?)
    }
}
