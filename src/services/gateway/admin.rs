use serde_json::{to_value, Value};

use crate::errors::AppError;

use super::HubuumGateway;

impl HubuumGateway {
    pub fn server_config(&self) -> Result<Value, AppError> {
        Ok(to_value(self.client.admin_config()?)?)
    }
}
