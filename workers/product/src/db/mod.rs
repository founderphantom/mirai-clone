use serde::de::DeserializeOwned;
use serde_json::Value;
use wasm_bindgen::JsValue;
use worker::{D1Database, Result as WorkerResult};

#[derive(Debug, Clone, PartialEq)]
enum D1Param {
    Null,
    String(String),
    Number(f64),
}

impl D1Param {
    fn into_js_value(self) -> JsValue {
        match self {
            Self::Null => JsValue::NULL,
            Self::String(value) => value.into(),
            Self::Number(value) => value.into(),
        }
    }
}

pub async fn first<T: DeserializeOwned>(
    db: &D1Database,
    sql: &str,
    params: Vec<Value>,
) -> WorkerResult<Option<T>> {
    let stmt = db.prepare(sql);
    let stmt = bind_values(stmt, params)?;
    stmt.first(None).await
}

pub async fn all<T: DeserializeOwned>(
    db: &D1Database,
    sql: &str,
    params: Vec<Value>,
) -> WorkerResult<Vec<T>> {
    let stmt = db.prepare(sql);
    let stmt = bind_values(stmt, params)?;
    let result = stmt.all().await?;
    Ok(result.results()?)
}

pub async fn exec(db: &D1Database, sql: &str, params: Vec<Value>) -> WorkerResult<()> {
    run(db, sql, params).await?;
    Ok(())
}

pub async fn run(db: &D1Database, sql: &str, params: Vec<Value>) -> WorkerResult<worker::D1Result> {
    let stmt = db.prepare(sql);
    let stmt = bind_values(stmt, params)?;
    stmt.run().await
}

pub async fn batch(
    db: &D1Database,
    statements: Vec<(&str, Vec<Value>)>,
) -> WorkerResult<Vec<worker::D1Result>> {
    let mut prepared = Vec::with_capacity(statements.len());
    for (sql, params) in statements {
        prepared.push(bind_values(db.prepare(sql), params)?);
    }
    db.batch(prepared).await
}

fn bind_values(
    stmt: worker::D1PreparedStatement,
    params: Vec<Value>,
) -> WorkerResult<worker::D1PreparedStatement> {
    let values = params_to_d1_params(params)
        .into_iter()
        .map(D1Param::into_js_value)
        .collect::<Vec<_>>();
    stmt.bind(&values)
}

fn params_to_d1_params(params: Vec<Value>) -> Vec<D1Param> {
    params
        .into_iter()
        .map(|value| match value {
            Value::Null => D1Param::Null,
            Value::String(value) => D1Param::String(value),
            Value::Number(value) => {
                if let Some(number) = value.as_f64() {
                    D1Param::Number(number)
                } else {
                    D1Param::String(value.to_string())
                }
            }
            Value::Bool(value) => D1Param::Number(if value { 1.0 } else { 0.0 }),
            other => D1Param::String(other.to_string()),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn converts_all_params_to_one_ordered_parameter_list() {
        let values = params_to_d1_params(vec![
            Value::String("creator".to_string()),
            Value::Number(42.into()),
            Value::Bool(true),
            Value::Null,
        ]);

        assert_eq!(
            values,
            vec![
                D1Param::String("creator".to_string()),
                D1Param::Number(42.0),
                D1Param::Number(1.0),
                D1Param::Null,
            ]
        );
    }
}
