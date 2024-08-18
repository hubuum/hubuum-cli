use crate::errors::AppError;
use std::any::type_name;

pub trait SingleItemOrWarning<T> {
    fn single_item_or_warning(self) -> Result<T, AppError>;
}

impl<T: Clone> SingleItemOrWarning<T> for Vec<T> {
    fn single_item_or_warning(self) -> Result<T, AppError> {
        let type_str = get_type_name::<T>();

        if self.is_empty() {
            Err(AppError::EntityNotFound(format!("{} not found", type_str)))
        } else if self.len() > 1 {
            Err(AppError::MultipleEntitiesFound(format!(
                "More than one {} found",
                type_str
            )))
        } else {
            Ok(self.into_iter().next().unwrap())
        }
    }
}

fn get_type_name<T>() -> &'static str {
    let full_type_name = type_name::<T>();
    full_type_name.rsplit("::").next().unwrap_or(full_type_name)
}
