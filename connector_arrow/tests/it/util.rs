use std::{fs::File, path::Path, sync::Arc};

use arrow::{
    datatypes::{DataType, Field, Schema},
    error::ArrowError,
    record_batch::RecordBatch,
};
use connector_arrow::{
    api::{Append, Connection, EditSchema, Statement},
    ConnectorError,
};
use itertools::Itertools;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;

#[track_caller]
pub fn load_parquet_if_not_exists<C>(conn: &mut C, file_path: &Path) -> (String, Vec<RecordBatch>)
where
    C: Connection + EditSchema,
{
    // read from file
    let arrow_file: Vec<RecordBatch> = {
        let file = File::open(file_path).unwrap();

        let builder = ParquetRecordBatchReaderBuilder::try_new(file).unwrap();

        let reader = builder.build().unwrap();
        reader.collect::<Result<Vec<_>, ArrowError>>().unwrap()
    };

    let schema = arrow_file.first().unwrap().schema();

    // table create
    let table_name = file_path.file_name().unwrap().to_str().unwrap().to_string();
    match conn.table_create(&table_name, schema) {
        Ok(_) => (),
        Err(connector_arrow::TableCreateError::TableExists) => return (table_name, arrow_file),
        Err(e) => panic!("{}", e),
    }

    // write into table
    {
        let mut appender = conn.append(&table_name).unwrap();
        for batch in arrow_file.clone() {
            appender.append(batch).unwrap();
        }
        appender.finish().unwrap();
    }

    (table_name, arrow_file)
}

#[track_caller]
pub fn roundtrip_of_parquet<C, F>(conn: &mut C, file_path: &Path, coerce_ty: F)
where
    C: Connection + EditSchema,
    F: Fn(&DataType) -> Option<DataType>,
{
    let (table_name, arrow_file) = load_parquet_if_not_exists(conn, file_path);

    // read from table
    let arrow_table = {
        let mut stmt = conn
            .query(&format!("SELECT * FROM \"{table_name}\""))
            .unwrap();
        let reader = stmt.start(()).unwrap();

        reader.collect::<Result<Vec<_>, ConnectorError>>().unwrap()
    };

    // table drop
    conn.table_drop(&table_name).unwrap();

    let arrow_file_coerced = cast_batches(&arrow_file, coerce_ty);
    similar_asserts::assert_eq!(&arrow_file_coerced, &arrow_table);
}

fn cast_batches<F>(batches: &[RecordBatch], coerce_ty: F) -> Vec<RecordBatch>
where
    F: Fn(&DataType) -> Option<DataType>,
{
    let arrow_file = batches
        .iter()
        .map(|batch| {
            let new_schema = Arc::new(Schema::new(
                batch
                    .schema()
                    .fields()
                    .iter()
                    .map(|f| match coerce_ty(f.data_type()) {
                        Some(new_ty) => Field::new(f.name(), new_ty, f.is_nullable()),
                        None => Field::clone(f),
                    })
                    .collect_vec(),
            ));

            let new_columns = batch
                .columns()
                .iter()
                .map(|col_array| match coerce_ty(col_array.data_type()) {
                    Some(new_ty) => arrow::compute::cast(&col_array, &new_ty).unwrap(),
                    None => col_array.clone(),
                })
                .collect_vec();

            RecordBatch::try_new(new_schema, new_columns).unwrap()
        })
        .collect_vec();
    arrow_file
}