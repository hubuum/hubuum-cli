use hubuum_client::{ClassFieldInventory, FieldDiscoveryOptions};

use crate::diagnostics::client_call;
use crate::domain::ResourceName;
use crate::errors::AppError;

use super::HubuumGateway;

impl HubuumGateway {
    pub(crate) fn discover_class_fields(
        &self,
        class_name: &ResourceName,
        options: &FieldDiscoveryOptions,
    ) -> Result<ClassFieldInventory, AppError> {
        Ok(client_call("classes.fields", || {
            self.client()
                .class_by_name(class_name.as_str())
                .fields(options)
        })?)
    }
}
