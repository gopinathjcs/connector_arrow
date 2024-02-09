//! Source implementation for DuckDB embedded database.
mod append;
mod schema;

use arrow::record_batch::RecordBatch;
use duckdb::{Appender, Arrow};

use std::sync::Arc;

use crate::api::{Connection, ResultReader, Statement};
use crate::errors::ConnectorError;

impl Connection for duckdb::Connection {
    type Stmt<'conn> = DuckDBStatement<'conn>
    where
        Self: 'conn;

    type Append<'conn> = Appender<'conn> where Self: 'conn;

    fn query<'a>(&'a mut self, query: &str) -> Result<Self::Stmt<'a>, ConnectorError> {
        let stmt = duckdb::Connection::prepare(self, query)?;

        Ok(DuckDBStatement { stmt })
    }
    fn append<'a>(&'a mut self, table_name: &str) -> Result<Self::Append<'a>, ConnectorError> {
        Ok(self.appender(table_name)?)
    }
}

pub struct DuckDBStatement<'conn> {
    stmt: duckdb::Statement<'conn>,
}

impl<'conn> Statement<'conn> for DuckDBStatement<'conn> {
    type Params = ();

    type Reader<'stmt> = DuckDBReader<'stmt>
    where
        Self: 'stmt;

    fn start(&mut self, _params: ()) -> Result<Self::Reader<'_>, ConnectorError> {
        let arrow = self.stmt.query_arrow([])?;
        Ok(DuckDBReader { arrow })
    }
}

pub struct DuckDBReader<'stmt> {
    arrow: Arrow<'stmt>,
}

impl<'stmt> ResultReader<'stmt> for DuckDBReader<'stmt> {
    fn get_schema(&mut self) -> Result<Arc<arrow::datatypes::Schema>, ConnectorError> {
        Ok(self.arrow.get_schema())
    }
}

impl<'stmt> Iterator for DuckDBReader<'stmt> {
    type Item = Result<RecordBatch, ConnectorError>;

    fn next(&mut self) -> Option<Self::Item> {
        self.arrow.next().map(Ok)
    }
}