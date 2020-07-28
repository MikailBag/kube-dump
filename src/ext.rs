use async_trait::async_trait;
use http::Request;
use k8s_openapi::apimachinery::pkg::apis::meta::v1 as metav1;

/// useful things that should be upstreamed to kube-rs
#[async_trait]
pub trait ClientExt {
    async fn cluster_version(&self) -> kube::Result<k8s_openapi::apimachinery::pkg::version::Info>;
    async fn list_api_groups(&self) -> kube::Result<metav1::APIGroupList>;
    async fn list_api_group_resources(
        &self,
        group: &str,
        version: &str,
    ) -> kube::Result<metav1::APIResourceList>;
    async fn list_legacy_api_versions(&self) -> kube::Result<metav1::APIVersions>;
    async fn list_legacy_api_resources(
        &self,
        version: &str,
    ) -> kube::Result<metav1::APIResourceList>;
}

#[async_trait]
impl ClientExt for kube::Client {
    async fn cluster_version(&self) -> kube::Result<k8s_openapi::apimachinery::pkg::version::Info> {
        self.request(Request::builder().uri("/version").body(Vec::new())?)
            .await
    }

    async fn list_api_groups(&self) -> kube::Result<metav1::APIGroupList> {
        self.request(Request::builder().uri("/apis").body(Vec::new())?)
            .await
    }

    async fn list_api_group_resources(
        &self,
        group: &str,
        version: &str,
    ) -> kube::Result<metav1::APIResourceList> {
        let url = format!("/apis/{}/{}", group, version);
        self.request(Request::builder().uri(url).body(Vec::new())?)
            .await
    }

    async fn list_legacy_api_versions(&self) -> kube::Result<metav1::APIVersions> {
        self.request(Request::builder().uri("/api").body(Vec::new())?)
            .await
    }

    async fn list_legacy_api_resources(
        &self,
        version: &str,
    ) -> kube::Result<metav1::APIResourceList> {
        let url = format!("/api/{}", version);
        self.request(Request::builder().uri(url).body(Vec::new())?)
            .await
    }
}
