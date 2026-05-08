use serde::de::DeserializeOwned;
use serde_json::Value;
use worker::{D1Database, Result as WorkerResult};

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
    let stmt = db.prepare(sql);
    let stmt = bind_values(stmt, params)?;
    stmt.run().await?;
    Ok(())
}

fn bind_values(
    mut stmt: worker::D1PreparedStatement,
    params: Vec<Value>,
) -> WorkerResult<worker::D1PreparedStatement> {
    for value in params {
        stmt = match value {
            Value::Null => stmt.bind(&[wasm_bindgen::JsValue::NULL])?,
            Value::String(value) => stmt.bind(&[value.into()])?,
            Value::Number(value) => {
                if let Some(number) = value.as_f64() {
                    stmt.bind(&[number.into()])?
                } else {
                    stmt.bind(&[value.to_string().into()])?
                }
            }
            Value::Bool(value) => stmt.bind(&[(if value { 1 } else { 0 }).into()])?,
            other => stmt.bind(&[other.to_string().into()])?,
        };
    }
    Ok(stmt)
}
