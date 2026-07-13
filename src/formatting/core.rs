use serde::Serialize;
use serde_json::{json, to_value, Map, Value};

use hubuum_filter::OutputEnvelope;

use crate::{errors::AppError, output::set_semantic_output};

pub trait OutputFormatter: Sized + Serialize + Clone {
    fn format(&self) -> Result<Self, AppError>;

    fn format_noreturn(&self) -> Result<(), AppError> {
        self.format()?;
        Ok(())
    }

    fn format_json_noreturn(&self) -> Result<(), AppError> {
        append_json(self)?;
        Ok(())
    }
}

pub trait DetailRenderable {
    fn detail_rows(&self) -> Vec<(&'static str, String)>;
}

pub trait TableRenderable {
    fn headers() -> Vec<&'static str>;
    fn row(&self) -> Vec<String>;
}

impl<T> OutputFormatter for T
where
    T: DetailRenderable + Serialize + Clone,
{
    fn format(&self) -> Result<Self, AppError> {
        let mut object = Map::new();
        let mut columns = Vec::new();
        for (key, value) in self.detail_rows() {
            columns.push(key.to_string());
            object.insert(key.to_string(), Value::String(value));
        }
        set_semantic_output(OutputEnvelope::detail(Value::Object(object), columns))?;
        Ok(self.clone())
    }
}

impl<T> OutputFormatter for Vec<T>
where
    T: TableRenderable + Serialize + Clone,
{
    fn format(&self) -> Result<Self, AppError> {
        let columns = T::headers()
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>();
        let rows = self
            .iter()
            .map(|row| {
                let mut object = match to_value(row)? {
                    Value::Object(object) => object,
                    value => {
                        let mut object = Map::new();
                        object.insert("value".to_string(), value);
                        object
                    }
                };
                for (column, value) in columns.iter().zip(row.row()) {
                    object.insert(column.clone(), Value::String(value));
                }
                Ok(Value::Object(object))
            })
            .collect::<Result<Vec<_>, AppError>>()?;
        set_semantic_output(OutputEnvelope::rows(rows, columns))?;
        Ok(self.clone())
    }
}

pub fn append_json_message<V>(value: V) -> Result<(), AppError>
where
    V: Serialize,
{
    let message = json!({ "message": value });
    set_semantic_output(OutputEnvelope::message(message))?;
    Ok(())
}

pub fn append_json<T>(value: &T) -> Result<(), AppError>
where
    T: Serialize + ?Sized,
{
    set_semantic_output(OutputEnvelope::detail(to_value(value)?, Vec::new()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{OutputFormatter, TableRenderable};
    use crate::{
        config::{init_config, AppConfig},
        models::{EmptyResult, TableStyle},
        output::{reset_output, set_pipeline, take_output},
    };
    use hubuum_filter::PipeStage;
    use serde::Serialize;
    use serial_test::serial;

    #[derive(Clone, Serialize)]
    struct Row {
        name: &'static str,
        value: &'static str,
        hidden: &'static str,
    }

    impl TableRenderable for Row {
        fn headers() -> Vec<&'static str> {
            vec!["name", "value"]
        }

        fn row(&self) -> Vec<String> {
            vec![self.name.to_string(), self.value.to_string()]
        }
    }

    #[test]
    #[serial]
    fn empty_table_can_be_silent() {
        let mut config = AppConfig::default();
        config.output.empty_result = EmptyResult::Silent;
        init_config(config).expect("config should initialize");
        reset_output().expect("output should reset");

        Vec::<Row>::new().format_noreturn().expect("format");

        assert!(take_output().expect("snapshot").is_empty());
    }

    #[test]
    #[serial]
    fn plain_table_style_removes_borders() {
        let mut config = AppConfig::default();
        config.output.table_style = TableStyle::Plain;
        init_config(config).expect("config should initialize");
        reset_output().expect("output should reset");

        vec![Row {
            name: "alpha",
            value: "one",
            hidden: "secret",
        }]
        .format_noreturn()
        .expect("format");

        let rendered = take_output().expect("snapshot").render();
        assert!(rendered.contains("alpha"));
        assert!(!rendered.contains('+'));
        assert!(!rendered.contains('│'));
    }

    #[test]
    #[serial]
    fn table_pipe_filter_matches_visible_fields_and_shows_hit_column() {
        init_config(AppConfig::default()).expect("config should initialize");
        reset_output().expect("output should reset");
        set_pipeline(vec![PipeStage::Grep("one".to_string())]).expect("pipeline should set");

        vec![
            Row {
                name: "alpha",
                value: "one",
                hidden: "cpu eko payload",
            },
            Row {
                name: "beta",
                value: "two",
                hidden: "other payload",
            },
        ]
        .format_noreturn()
        .expect("format");

        let rendered = take_output().expect("snapshot").render();
        assert!(rendered.contains("alpha"));
        assert!(!rendered.contains("beta"));
        assert!(rendered.contains("Match"));
        assert!(rendered.contains("value"));
        assert!(!rendered.contains("cpu eko payload"));
    }
}
