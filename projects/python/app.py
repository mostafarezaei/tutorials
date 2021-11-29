import boto3
import tempfile
import os
import base64
import logging
import psycopg2
import yaml
import hashlib
from random import randrange
from botocore.exceptions import NoCredentialsError, ClientError

# To perform the data encryption/decryption operations,
# the Python cryptography package must be installed.
#       pip install cryptography
from cryptography.fernet import Fernet

RANDOM_USER_NAMES = ['Liam', 'Noah', 'Oliver', 'Elijah', 'William', 'James',
                     'Benjamin', 'Lucas', 'Henry', 'Alexander', 'Mason',
                     'Michael', 'Ethan', 'Daniel', 'Jacob']
config = yaml.safe_load(open("config.yaml"))


def get_db_conn():
    try:
        dbConfig = config['database']
        conn_string = "host={host} port={port} dbname={dbname} user={user} password={password}"
        conn_string = conn_string.format(
            host=dbConfig['host'],
            port=dbConfig['port'],
            dbname=dbConfig['dbname'],
            user=dbConfig['user'],
            password=dbConfig['password']
        )
        return psycopg2.connect(conn_string)
    except yaml.YAMLError as e:
        logging.error(e)


def retrieve_cmk(desc):
    """Retrieve an existing KMS CMK based on its description

    :param desc: Description of CMK specified when the CMK was created
    :return Tuple(KeyId, KeyArn) where:
        KeyId: CMK ID
        KeyArn: Amazon Resource Name of CMK
    :return Tuple(None, None) if a CMK with the specified description was
    not found
    """

    # Retrieve a list of existing CMKs
    # If more than 100 keys exist, retrieve and process them in batches
    kms_client = boto3.client('kms')
    try:
        response = kms_client.list_keys()
    except ClientError as e:
        logging.error(e)
        return None, None

    done = False
    while not done:
        for cmk in response['Keys']:
            # Get info about the key, including its description
            try:
                key_info = kms_client.describe_key(KeyId=cmk['KeyArn'])
            except ClientError as e:
                logging.error(e)
                return None, None

            # Is this the key we're looking for?
            if key_info['KeyMetadata']['Description'] == desc:
                return cmk['KeyId'], cmk['KeyArn']

        # Are there more keys to retrieve?
        if not response['Truncated']:
            # No, the CMK was not found
            logging.debug('A CMK with the specified description was not found')
            done = True
        else:
            # Yes, retrieve another batch
            try:
                response = kms_client.list_keys(Marker=response['NextMarker'])
            except ClientError as e:
                logging.error(e)
                return None, None

    # All existing CMKs were checked and the desired key was not found
    return None, None


def create_data_key(cmk_id, key_spec='AES_256'):
    """Generate a data key to use when encrypting and decrypting data

    :param cmk_id: KMS CMK ID or ARN under which to generate and encrypt the
    data key.
    :param key_spec: Length of the data encryption key. Supported values:
        'AES_128': Generate a 128-bit symmetric key
        'AES_256': Generate a 256-bit symmetric key
    :return Tuple(EncryptedDataKey, PlaintextDataKey) where:
        EncryptedDataKey: Encrypted CiphertextBlob data key as binary string
        PlaintextDataKey: Plaintext base64-encoded data key as binary string
    :return Tuple(None, None) if error
    """

    # Create data key
    kms_client = boto3.client('kms')
    try:
        response = kms_client.generate_data_key(KeyId=cmk_id, KeySpec=key_spec)
    except ClientError as e:
        logging.error(e)
        return None, None

    # Return the encrypted and plaintext data key
    return response['CiphertextBlob'], base64.b64encode(response['Plaintext'])


def decrypt_data_key(data_key_encrypted):
    """Decrypt an encrypted data key

    :param data_key_encrypted: Encrypted ciphertext data key.
    :return Plaintext base64-encoded binary data key as binary string
    :return None if error
    """

    # Decrypt the data key
    kms_client = boto3.client('kms')
    try:
        response = kms_client.decrypt(CiphertextBlob=data_key_encrypted)
    except ClientError as e:
        logging.error(e)
        return None

    # Return plaintext base64-encoded binary data key
    return base64.b64encode((response['Plaintext']))


def upload_data_to_s3(data, bucket, s3_file_name):
    with tempfile.TemporaryDirectory() as tmp:
        local_temp_file = os.path.join(tmp, 'python.out')
        with open(local_temp_file, 'w') as f:
            f.write(data)

        s3 = boto3.client('s3', aws_access_key_id=config['aws']['accessKey'],
                          aws_secret_access_key=config['aws']['secretKey'])

        try:
            s3.upload_file(local_temp_file, bucket, s3_file_name)
            logging.info("File {} Uploaded Successfully into S3 bucket named {}"
                         .format(s3_file_name, bucket))
            return True
        except FileNotFoundError:
            logging.error("An ERROR happen uploading file into S3")
            return False
        except NoCredentialsError:
            print("AWS Credentials not available")
            return False


def store_encrypted_random_data(data_key_id, datakey_plaintext):
    """Encrypt data using an AWS KMS CMK

    A data key is generated and associated with the CMK.
    The encrypted data key is saved in database. This enables
    data to be decrypted at any time in the future and by any
    program that has the credentials to decrypt the data key.

    :param data_key_id: The data key id stored in Table encrypted_data_keys.
    :param datakey_plaintext: unencrypted data key used to code sensitive data.
    """

    fernet = Fernet(datakey_plaintext)

    conn = get_db_conn()
    cur = conn.cursor()
    try:
        for i in range(15):
            enc_sensitive_data = fernet.encrypt(
                randrange(10000).to_bytes(4, byteorder='big'))
            user_name = RANDOM_USER_NAMES[i]
            cur.execute("""
                DO LANGUAGE plpgsql $$ DECLARE
                BEGIN
                    IF NOT EXISTS (SELECT * FROM public.data
                                WHERE user_name = '%s') THEN
                        INSERT INTO data(user_name, sensitive_data, data_key_id)
                        VALUES('%s', %s, %s);
                    END IF;
                END $$;
            """ % (user_name, user_name, psycopg2.Binary(enc_sensitive_data), data_key_id))
            conn.commit()
        cur.close()
    except (Exception, psycopg2.DatabaseError) as error:
        logging.error(error)
    finally:
        if conn is not None:
            conn.close()

    # For the highest security, the datakey_plaintext value should be wiped
    # from memory. Unfortunately, this is not possible in Python. However,
    # storing the value in a local variable makes it available for garbage
    # collection.


def decrypt_sensitive_data_and_sum_it():
    """Retreives and decrypts sensitive_data field from data table records.
        In the next step, add all these values together and return.

       NOTE: There is no need to know the cipher data key to decode encrypted data.
             Each record is connected to its own cipher data key through a foreign key.
             Note that each data record can be encrypted with different data keys.
    """
    sum = 0
    conn = None
    try:
        conn = get_db_conn()
        cur = conn.cursor()
        cur.execute("""
            SELECT user_name, sensitive_data::bytea, data_key::bytea
            FROM data d INNER JOIN encrypted_data_keys e
            ON d.data_key_id=e.id
        """)
        row = cur.fetchone()
        fernet = None

        # Data keys may be different for each record
        last_data_key_digest = None

        while row is not None:
            cipher_data_key = row[2].tobytes()
            if last_data_key_digest is None or hashlib.sha1(cipher_data_key).hexdigest() != last_data_key_digest:
                # anothet data key, so recompute plain data key.
                # Decrypt the data key before using it
                decrypted_data_key = decrypt_data_key(cipher_data_key)
                last_data_key_digest = hashlib.sha1(
                    cipher_data_key).hexdigest()

            if decrypted_data_key is not None:
                fernet = Fernet(decrypted_data_key)
            else:
                logging.error("Cannot decrypt the data key")
                return

            sensitive_data = int.from_bytes(
                fernet.decrypt(row[1].tobytes()), "big")
            logging.info("{}'s sensitive data is {}.".format(
                row[0].strip(), sensitive_data))
            sum += sensitive_data

            row = cur.fetchone()

        cur.close()
    except (Exception, psycopg2.DatabaseError) as error:
        logging.error(error)
    finally:
        if conn is not None:
            conn.close()

    # The same security issue described at the end of store_encrypted_random_data()
    # exists here, too, i.e., the wish to wipe the data_key_plaintext value from
    # memory.
    return sum


def gen_datakey_and_store_in_db(cmk_desc, cmk_id, cmk_arn):

    # Generate a data key associated with the CMK
    # The data key is used to encrypt data. Each record
    # can use its own data key or data keys can be shared among them.
    # Specify either the CMK ID or ARN

    data_key_encrypted, data_key_plaintext = create_data_key(cmk_id)
    if data_key_encrypted is None:
        return False
    logging.info('Created new AWS KMS data key')

    conn = None
    try:
        conn = get_db_conn()
        cur = conn.cursor()
        cur.execute("""
            DO LANGUAGE plpgsql $$ DECLARE
            BEGIN
                IF NOT EXISTS (SELECT * FROM public.encrypted_data_keys
                            WHERE key_desc = '%s') THEN
                    INSERT INTO encrypted_data_keys(data_key, key_desc)
                    VALUES(%s, '%s');
                END IF;
            END $$;
            SELECT ID FROM public.encrypted_data_keys WHERE key_desc='%s';
        """ % (cmk_desc, psycopg2.Binary(data_key_encrypted), cmk_desc, cmk_desc))
        cipher_data_key_id = cur.fetchone()[0]

        conn.commit()
        cur.close()
    except (Exception, psycopg2.DatabaseError) as error:
        logging.error(error)
    finally:
        if conn is not None:
            conn.close()

    return cipher_data_key_id, data_key_plaintext


if __name__ == '__main__':

    # Set up logging
    logging.basicConfig(level=logging.INFO,
                        format='%(levelname)s: %(asctime)s: %(message)s')

    # Specify the description for the CMK. A key with this description
    # has already been created in kms. We retrieve it and use it to
    # encrypt and encrypt data.
    cmk_desc = 'tutCMK'

    # Does the desired CMK already exist?
    cmk_id, cmk_arn = retrieve_cmk(cmk_desc)
    if cmk_id is not None:
        logging.info('Retrieved existing AWS KMS CMK')
    else:
        logging.info('AWS KMS CMK not found!')
        exit(1)

    # As you know, a data key is created from cmk to encrypt data.
    # An encrypted data key is also written to output encrypted data.
    # The encrypted data can be decrypted at any time and by any program
    # that has the credentials to decrypt the data key.

    datakey_id, datakey_plaintext = gen_datakey_and_store_in_db(
        cmk_desc, cmk_id, cmk_arn)

    # First store some encrypted data on db
    store_encrypted_random_data(datakey_id, datakey_plaintext)

    # Now we retreive the encrypted data from the database
    # and after decrypting the sensitive data field, we sum it together.
    sum_value = decrypt_sensitive_data_and_sum_it()

    # Upload the result into a file in S3 bucket.
    uploaded = upload_data_to_s3(
        str(sum_value), config['s3']['outputBucketName'],
        config['s3']['outputFileBaseName']+'-ByPython')
