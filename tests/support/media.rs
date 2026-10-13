//! Isolated real SeaweedFS buckets for media integration tests, with owned-resource cleanup.

use aws_sdk_s3::{
    Client,
    config::{
        BehaviorVersion, Credentials, Region, RequestChecksumCalculation,
        ResponseChecksumValidation,
    },
};
use chat_server::infrastructure::config::{Config, MediaStorageConfig};

pub struct TestStorage {
    pub sdk: Client,
    pub bucket: String,
}

impl Drop for TestStorage {
    fn drop(&mut self) {
        let sdk = self.sdk.clone();
        let bucket = self.bucket.clone();
        assert!(bucket.starts_with("media-test-"));
        std::thread::spawn(move || {
            tokio::runtime::Runtime::new()
                .unwrap()
                .block_on(async move {
                    let cleanup = async {
                        loop {
                            let page = sdk.list_objects_v2().bucket(&bucket).send().await?;
                            if page.contents().is_empty() {
                                break;
                            }
                            for object in page.contents() {
                                if let Some(key) = object.key() {
                                    sdk.delete_object()
                                        .bucket(&bucket)
                                        .key(key)
                                        .send()
                                        .await
                                        .map_err(anyhow::Error::from)?;
                                }
                            }
                        }
                        sdk.delete_bucket()
                            .bucket(&bucket)
                            .send()
                            .await
                            .map_err(anyhow::Error::from)?;
                        Ok::<_, anyhow::Error>(())
                    };
                    if !matches!(
                        tokio::time::timeout(std::time::Duration::from_secs(30), cleanup).await,
                        Ok(Ok(()))
                    ) {
                        eprintln!("owned media test bucket cleanup failed");
                    }
                });
        });
    }
}

pub async fn setup(quota: u64) -> (super::TestApp, TestStorage) {
    let mut config = Config::from_env().unwrap();
    let mut storage: MediaStorageConfig = config
        .media_storage
        .clone()
        .expect("local media credentials required");
    storage.bucket = format!("media-test-{}", uuid::Uuid::new_v4().simple());
    storage.public_endpoint = storage.internal_endpoint.clone();
    let sdk = Client::from_conf(
        aws_sdk_s3::Config::builder()
            .behavior_version(BehaviorVersion::latest())
            .credentials_provider(Credentials::new(
                &storage.access_key,
                &storage.secret_key,
                None,
                None,
                "media-test",
            ))
            .region(Region::new(storage.region.clone()))
            .endpoint_url(&storage.internal_endpoint)
            .force_path_style(true)
            .request_checksum_calculation(RequestChecksumCalculation::WhenRequired)
            .response_checksum_validation(ResponseChecksumValidation::WhenRequired)
            .build(),
    );
    sdk.create_bucket()
        .bucket(&storage.bucket)
        .send()
        .await
        .expect("real SeaweedFS bucket bootstrap");
    let owned = TestStorage {
        sdk,
        bucket: storage.bucket.clone(),
    };
    config.media_storage = Some(storage);
    config.media_user_quota_bytes = quota;
    (super::spawn_test_app_with_config(config).await, owned)
}
