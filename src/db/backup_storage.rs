use anyhow::{Context, Result, bail};
use rusqlite::{OptionalExtension, params};

use super::Db;
use crate::backup_storage::{BackupStorageConfig, StorageProvider};

pub fn get(db: &Db, instance_name: &str) -> Result<Option<BackupStorageConfig>> {
    let row = db
        .conn()
        .query_row(
            "SELECT provider, endpoint, region, bucket, prefix, access_key_id, \
                    secret_access_key, enabled \
             FROM backup_storage_configs WHERE instance_name = ?1",
            params![instance_name],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, bool>(7)?,
                ))
            },
        )
        .optional()
        .with_context(|| format!("failed to load backup storage for '{instance_name}'"))?;

    let Some((
        provider,
        endpoint,
        region,
        bucket,
        prefix,
        access_key_id,
        secret_access_key,
        enabled,
    )) = row
    else {
        return Ok(None);
    };
    let Some(provider) = StorageProvider::from_db(&provider) else {
        bail!("unknown backup storage provider '{provider}' for '{instance_name}'");
    };
    Ok(Some(BackupStorageConfig {
        provider,
        endpoint,
        region,
        bucket,
        prefix,
        access_key_id,
        secret_access_key,
        enabled,
    }))
}

pub fn upsert(db: &Db, instance_name: &str, config: &BackupStorageConfig) -> Result<()> {
    db.conn()
        .execute(
            "INSERT INTO backup_storage_configs \
                (instance_name, provider, endpoint, region, bucket, prefix, access_key_id, \
                 secret_access_key, enabled) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) \
             ON CONFLICT(instance_name) DO UPDATE SET \
                provider = excluded.provider, \
                endpoint = excluded.endpoint, \
                region = excluded.region, \
                bucket = excluded.bucket, \
                prefix = excluded.prefix, \
                access_key_id = excluded.access_key_id, \
                secret_access_key = excluded.secret_access_key, \
                enabled = excluded.enabled",
            params![
                instance_name,
                config.provider.as_db(),
                config.endpoint,
                config.region,
                config.bucket,
                config.prefix,
                config.access_key_id,
                config.secret_access_key,
                config.enabled,
            ],
        )
        .with_context(|| format!("failed to save backup storage for '{instance_name}'"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::instance::state::InstanceState;
    use crate::paths::Paths;

    fn temp_db(label: &str) -> Db {
        let dir = std::env::temp_dir().join(format!(
            "odin-db-backup-storage-test-{label}-{}-{}",
            std::process::id(),
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let db = Db::open(&Paths {
            data_dir: dir.clone(),
            config_dir: dir,
        })
        .unwrap();
        crate::db::instances::save(&db, &InstanceState::new("my-server", 2456)).unwrap();
        db
    }

    #[test]
    fn upsert_then_get_round_trips() {
        let db = temp_db("roundtrip");
        let config = BackupStorageConfig {
            provider: StorageProvider::CloudflareR2,
            endpoint: "https://account.r2.cloudflarestorage.com".to_string(),
            region: "auto".to_string(),
            bucket: "odin-backups".to_string(),
            prefix: "valheim".to_string(),
            access_key_id: "access".to_string(),
            secret_access_key: "secret".to_string(),
            enabled: true,
        };

        upsert(&db, "my-server", &config).unwrap();

        assert_eq!(get(&db, "my-server").unwrap(), Some(config));
    }
}
