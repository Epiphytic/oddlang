use std::time::{Duration, SystemTime, UNIX_EPOCH};

use postgres::{Client, Config, NoTls};

const DATABASE_URL_ENV: &str = "ODDLANG_TEST_DATABASE_URL";
const SERVER_VERSION: &str = "160006";

pub struct TestDatabase {
    pub client: Client,
    pub schema: String,
}

impl TestDatabase {
    pub fn connect() -> Result<Self, String> {
        let url = std::env::var(DATABASE_URL_ENV).map_err(|error| {
            format!("database setup: set {DATABASE_URL_ENV} to the test database URL ({error})")
        })?;
        let mut config = url
            .parse::<Config>()
            .map_err(|error| format!("database setup: invalid {DATABASE_URL_ENV}: {error}"))?;
        let mut client = config
            .connect_timeout(Duration::from_secs(5))
            .connect(NoTls)
            .map_err(|error| {
                format!("database setup: could not connect using {DATABASE_URL_ENV}: {error}")
            })?;
        let server_info = client
            .query_one(
                "SELECT current_setting('server_version_num'), pg_backend_pid()",
                &[],
            )
            .map_err(|error| format!("database setup: could not inspect PostgreSQL: {error}"))?;
        validate_server_version(server_info.get(0))?;

        // The backend PID distinguishes simultaneous sessions. The timestamp also
        // distinguishes reused PIDs if a previous process left a schema behind.
        let backend_pid: i32 = server_info.get(1);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| format!("database setup: invalid system time: {error}"))?
            .as_nanos();
        let schema = format!("oddlang_test_{backend_pid}_{timestamp}");
        client
            .batch_execute(&format!(
                "CREATE SCHEMA {schema}; SET search_path TO {schema};"
            ))
            .map_err(|error| format!("database setup: could not initialize schema: {error}"))?;

        Ok(Self { client, schema })
    }
}

pub(crate) fn validate_server_version(actual: &str) -> Result<(), String> {
    if actual != SERVER_VERSION {
        return Err(format!(
            "database setup: expected PostgreSQL 16.6 (server_version_num {SERVER_VERSION}), found {actual}"
        ));
    }
    Ok(())
}

impl Drop for TestDatabase {
    fn drop(&mut self) {
        // Reset any open or failed transaction before dropping the schema.
        let result = self.client.batch_execute("ROLLBACK").and_then(|()| {
            self.client
                .batch_execute(&format!("DROP SCHEMA {} CASCADE", self.schema))
        });
        if let Err(error) = result {
            let message = format!("could not clean up test schema {}: {error}", self.schema);
            if std::thread::panicking() {
                eprintln!("{message}");
            } else {
                panic!("{message}");
            }
        }
    }
}
