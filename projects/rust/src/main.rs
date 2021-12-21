#[macro_use]
extern crate lazy_static;
extern crate yaml_rust;

use std::fmt::Write;
use std::fs::File;
use std::io::{Read, Write as io_write};
use std::ops::Add;

use arrayvec::ArrayVec;
use aws_config::meta::region::RegionProviderChain;
use aws_sdk_s3::{ByteStream, Error};
use fasthash::metro as FastHash;
use fernet::Fernet;
use kms::{Blob, Client, Region};
use kms::model::DataKeySpec;
use log::LevelFilter;
use random::Source;
use tempfile::NamedTempFile;
use tokio_postgres::{Client as PGClient, NoTls};
use yaml_rust::{Yaml, yaml};

use chrono::Local;

lazy_static! {
    static ref CONFIG: Yaml = { // yaml config file
        let mut s = String::new();
        File::open("config.yaml").expect("config.yaml file not found!")
            .read_to_string(&mut s).unwrap();
        return yaml::YamlLoader::load_from_str(&s)
            .expect("Error in config.yaml file format!")[0].clone();
    };
    static ref RANDOM_USER_NAMES: Vec<&'static str> = {
        vec!["Liam", "Noah", "Oliver", "Elijah", "William", "James", "Benjamin", "Lucas",
            "Henry", "Alexander", "Mason", "Michael", "Ethan", "Daniel", "Jacob"]};
}
///
/// This program is made as an example to show the efficiency of using
/// KMS and S3 services. The general procedure is that the data key is
/// first generated for the given cmk key using the KMS. This data key
/// is stored encrypted in the database. Now some random data with a
/// sample sensitive data is created and stored in database encrypted
/// using this data key. Obviously, the data key reference is stored
/// next to each field of the sensitive value. In the next step, The
/// data is retrieved and, for example, sensitive data values are added
/// together after decryption. The aggregate value for the sample is
/// loaded into a file on S3.
///
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // set the logger
    env_logger::Builder::new().format(|buf, record| {
        writeln!(buf, "{} [{}] - {}", Local::now().format("%Y-%m-%dT%H:%M:%S"), record.level(), record.args())
    }).filter(None, LevelFilter::Info).init();

    // get data key for out cmk from KMS and store in database.
    let (datakey_id, datakey_plaintext) = gen_datakey_and_store_in_db().await?;

    // generate some random data, encrypt them and store on database.
    store_encrypted_random_data(datakey_id, datakey_plaintext).await;

    // retrieve data, decrypting and calculating the sum.
    let a = decrypt_sensitive_data_and_sum_it().await.to_string();

    // now upload a file containing the sum value to S3.
    upload_data_to_s3(&a).await;

    Ok(())
}

/// Generate the data key and save it in the encrypted_data_keys table.
/// This function also returns the plain text data key so that the data
/// can be encrypted using it. Storing the encrypted value of the data key
/// in a table in the database and keeping the reference to it next to
/// the sensitive data field is so that each program can decrypt each record
/// independently of each other.
///
/// Returns a Result with a tuple containing:
///     1- an reference id to the stored encrypted data key in database
///     2- plain text data key to encode data with
///
async fn gen_datakey_and_store_in_db() -> Result<(i32, String), Error> {
    // which KMS key we want to use
    let cmk_name = String::new().add(CONFIG["aws"]["cmkName"].as_str().unwrap());

    // generate data key
    let (data_key_encrypted, data_key_plaintext) = create_data_key(&cmk_name).await.unwrap();

    // connection to database
    let client = db_connection().await.expect("Error connecting database!");
    let stmt = client.prepare("
        SELECT ID FROM public.encrypted_data_keys WHERE cmk_name = $1;
    ").await.expect("Error processing query!");

    // Check if a data key for this cmk has already been stored in the database
    let data_key_id: i64 = match client.query_opt(&stmt, &[&cmk_name]).await
        .expect("Error querying database!") {
        Some(row) => row.get(0), // if so, return it.
        None => { // if not, insert the new one
            let stmt = client.prepare("
               INSERT INTO encrypted_data_keys(data_key, cmk_name)
               VALUES($1, $2) RETURNING id;
            ").await.expect("Error processing query!");
            client.query_one(&stmt, &[&data_key_encrypted, &cmk_name]).await
                .expect("Error inserting new data key to database!").get(0)
        }
    };

    Ok((data_key_id as i32, data_key_plaintext))
}

/// Generate random data and encrypt data based on the data key given as an argument.
/// After this, the data is stored in the database. A data key is generated and
/// associated with the CMK. The encrypted data key is saved in database. This enables
/// data to be decrypted at any time in the future and by any program that has the
/// credentials to decrypt the data key.
///
/// # Arguments
///     `data_key_id`: The data key id stored in Table encrypted_data_keys.
///     'data_key_plain_text': unencrypted data key used to code sensitive data.
///
async fn store_encrypted_random_data(data_key_id: i32, data_key_plain_text: String) {
    // a randomizer to generate random data
    let mut random_source = random::default().seed([1000, 10000]);
    let client = db_connection().await.expect("Error connection database!");

    // an encryption lib used to encrypt data with the given data key
    let fernet = fernet::Fernet::new(&data_key_plain_text)
        .expect("Problem creating data encryption object!");

    // generate 15 random record
    for i in 0..15 {
        let sensitive_data = random_source.read::<i32>().abs();
        let enc_sensitive_data = fernet.encrypt(&sensitive_data.to_be_bytes());
        let user_name = RANDOM_USER_NAMES[i];
        let stmt = client.prepare("SELECT count(*) FROM public.data WHERE user_name = $1")
            .await.expect("Error processing query!");
        // check if there's any record with this username
        let row = client.query_one(&stmt, &[&user_name])
            .await.expect("Could not run database query!");
        if row.get::<usize, i64>(0) == 0 {
            // insert record into the table
            let stmt = client.prepare(r#"
                        INSERT INTO data(user_name, sensitive_data, data_key_id)
                        VALUES($1, $2, $3);
            "#).await.expect("Error processing database query!");

            // execute the actual command
            client.execute(&stmt,
                           &[&user_name, &enc_sensitive_data.as_bytes(), &(data_key_id as i64)])
                .await.expect("Could not run database query!");
        }
    }
}

/// Retrieves and decrypts sensitive_data field from data table records.
/// In the next step, add all these values together and return.
///
/// NOTE: There is no need to know the cipher data key to decode encrypted data.
/// Each record is connected to its own cipher data key through a foreign key.
/// Note that each data record can be encrypted with different data keys.
///
async fn decrypt_sensitive_data_and_sum_it() -> i64 {
    let mut sum = 0_i64;
    let mut fernet: Option<Fernet> = None;
    let mut decrypted_data_key;
    let mut last_data_key_digest = 0_u64;
    let kms_client = get_kms_client().await;
    let db_client = db_connection().await.expect("Error connecting database!");
    let cmk_name = CONFIG["aws"]["cmkName"].as_str().unwrap().to_string();

    // iterate through database records to do the job
    for row in db_client.query("
                SELECT user_name, sensitive_data::bytea, data_key
                FROM data d INNER JOIN encrypted_data_keys e
                ON d.data_key_id=e.id", &[]).await
        .expect("Error getting data from database!") {
        let cipher_data_key: Vec<u8> = row.get(2);

        // check if data key changed since last time
        if FastHash::hash64(cipher_data_key.as_slice()) != last_data_key_digest {
            // If so, the data key must be decrypted and a decoder made based on it
            decrypted_data_key = match decrypt_data_key(cipher_data_key.as_slice(), &cmk_name, &kms_client).await {
                Ok(ddk) => { ddk }
                Err(_) => { panic!("Error decrypting data key!"); }
            };
            last_data_key_digest = FastHash::hash64(&cipher_data_key);
            // the decoder to decode data after this
            fernet = Some(Fernet::new(base64::encode_config(&decrypted_data_key[..32], base64::URL_SAFE).as_str())
                .expect("Unable to make a decoder!"));
        }
        // sensitive data encrypted from bytes in db to a string
        let sensitive_enc_string = &String::from_utf8(row.get::<usize, Vec<u8>>(1))
            .expect("Error retrieving sensitive field value!");

        // decrypt data
        let array: ArrayVec<u8, 4> = fernet.as_ref().expect("No decode was selected!")
            .decrypt(sensitive_enc_string).expect("record could not be decrypted!")
            .into_iter().collect();

        // convert it to an integer to use
        let sensitive_data = u32::from_be_bytes(array.into_inner().unwrap());
        log::info!("{}'s sensitive data is {}.", row.get::<usize, String>(0).trim(), sensitive_data);

        // calculate the sum after all.
        sum += sensitive_data as i64;
    }
    sum
}

/// Asks kms to generate a data key for the cmk we want.
///
/// Returns a tuple containing:
///     1- encrypted data key to store on database
///     2- plain text data key to encrypt data with it.
///
/// # Arguments
///    * `cmk_id`: cmk id of kms key on which we want the data key to be generated.
///
async fn create_data_key(cmk_id: &String) -> Result<(Vec<u8>, String), kms::Error> {
    let kms_client = get_kms_client().await;
    let resp = kms_client
        .generate_data_key()
        .key_id(cmk_id)
        .key_spec(DataKeySpec::Aes256)
        .send().await
        .expect("Error generating cmk using KMS!");

    let blob = resp.ciphertext_blob.expect("Could not get encrypted text");
    let plain_data_key_vec = match resp.plaintext {
        Some(text) => text,
        None => panic!("Invalid data key from KMS!"),
    }.as_ref().to_vec();

    // The kms documentation states that to use the plain text key, it must first be coded as base64.
    let plain_text_data_key = base64::encode_config(&plain_data_key_vec[..32], base64::URL_SAFE);

    let ddk = Vec::from(blob.as_ref()); // the encrypted data key to store on database.
    log::info!("plained data key: {}", plain_text_data_key);
    Ok((ddk, plain_text_data_key))
}

/// Send encrypted data key to KMS and request to decrypt it
/// Returns a Result containing a binary vector of decrypted data key.
///
/// # Arguments
///    * `cipher_data_key`: Encrypted data key that we want to decrypt.
///    * `cmk_id`: cmk id for kms on which we want the data key to be decrypted.
///    * `client`: the KMS client to use.
///
async fn decrypt_data_key(cipher_data_key: &[u8], cmk_id: &String, client: &kms::Client) -> Result<Vec<u8>, Error> {
    let data = Blob::new(cipher_data_key);
    let resp = client
        .decrypt()
        .key_id(cmk_id)
        .ciphertext_blob(data)
        .send()
        .await.expect("Data key could not be decrypted by KMS!");

    Ok(resp.plaintext.unwrap().as_ref().to_vec())
}

/// Uploads the file containing the desired data to S3. The bucket name
/// and file name are removed from the config.yaml file.
///
/// # Arguments
///    * `data`: A string of data that must be written to the file.
///
async fn upload_data_to_s3(data: &String) {
    // Where should the AWS order be issued?
    let region_provider = RegionProviderChain::default_provider()
        .or_else(Region::new(CONFIG["aws"]["region"].as_str().unwrap()));

    // get aws config from aws cli environment.
    let shared_config = aws_config::from_env().region(region_provider).load().await;

    // S3 client to use
    let client = aws_sdk_s3::Client::new(&shared_config);

    // bucket to upload file into
    let bucket = &CONFIG["s3"]["outputBucketName"].as_str()
        .expect("outputBucketName not specified in config file!").to_string();

    // the file name
    let file_name = CONFIG["s3"]["outputFileBaseName"].as_str()
        .expect("outputFileBaseName not specified in config file!").to_string() + "-ByRust";

    // Use a temporary file to write data to it and create a stream on it to send to S3
    let mut temp_file = NamedTempFile::new().expect("could not create temp file!");
    temp_file.write(data.as_ref()).expect("Error writing temp file!");

    // a stream on temp file
    let body = ByteStream::from_path(&temp_file.path()).await
        .expect("Error reading from temp file!");

    // finally uploading the file
    client.put_object().bucket(String::from(bucket)).key(&file_name)
        .body(body).send().await.expect("Error putting file to S3");
    log::info!("File {} uploaded successfully to S3 bucket named {}", &file_name, &bucket);
}

/// Builds a kms client based on the desired config.
/// It should be noted that the config required to connect to aws is taken
/// from the aws cli environment.
async fn get_kms_client() -> Client {
    let region_provider = RegionProviderChain::default_provider()
        .or_else(Region::new(CONFIG["aws"]["region"].as_str().unwrap()));
    let shared_config = aws_config::from_env().region(region_provider).load().await;
    let client = kms::Client::new(&shared_config);
    client
}

/// Creates connection to database based on config.yaml configurations.
///
async fn db_connection() -> Result<PGClient, Error> {
    let mut conn_string = String::new();
    write!(conn_string, "host={} port={} dbname={} user={} password={}",
           CONFIG["database"]["host"].as_str().unwrap(),
           CONFIG["database"]["port"].as_str().unwrap(),
           CONFIG["database"]["dbname"].as_str().unwrap(),
           CONFIG["database"]["user"].as_str().unwrap(),
           CONFIG["database"]["password"].as_str().unwrap()).unwrap();

    // Connect to the database.
    let (client, connection) =
        tokio_postgres::connect(&conn_string, NoTls).await
            .expect("Connecting database failed! Maybe connection string is wrong.");

    // The connection object performs the actual communication with the database,
    // so spawn it off to run on its own.
    tokio::spawn(async move {
        if let Err(e) = connection.await {
            log::error!("Database connection could not be established! {}", e);
        }
    });

    Ok(client)
}
