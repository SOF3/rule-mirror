use serde::Deserialize;

#[derive(Deserialize)]
pub struct PingEvent {}

#[derive(Deserialize)]
pub struct InstallationEvent {
    pub action: InstallationEventAction,
    pub repositories: Vec<Repo>,
    pub installation: Installation,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallationEventAction {
    Created,
    Deleted,
    Suspend,
    Unsuspend,
    NewPermissionsAccepted,
}

impl InstallationEventAction {
    pub fn seen(self) -> bool {
        match self {
            Self::Created | Self::Unsuspend | Self::NewPermissionsAccepted => true,
            Self::Deleted | Self::Suspend => false,
        }
    }
}

#[derive(Deserialize)]
pub struct InstallationRepositoriesEvent {
    pub action: InstallationRepositoriesEventAction,
    pub repositories_added: Vec<Repo>,
    pub repositories_removed: Vec<Repo>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallationRepositoriesEventAction {
    Added,
    Removed,
}

impl InstallationRepositoriesEventAction {
    pub fn seen(self) -> bool {
        match self {
            Self::Added => true,
            Self::Removed => false,
        }
    }
}

#[derive(Deserialize)]
pub struct PushEvent {
    pub installation: Installation,
    pub repository: Repo,
    #[serde(rename = "ref")]
    pub ref_: String,
}

#[derive(Deserialize)]
pub struct RepoEvent {
    pub action: RepoEventAction,
    pub repository: Repo,
    pub installation: Installation,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepoEventAction {
    Created,
    Deleted,
    Archived,
    Unarchived,
    Edited,
    Renamed,
    Transferred,
    Publicized,
    Privatized,
}

impl RepoEventAction {
    pub fn seen(self) -> Option<bool> {
        match self {
            Self::Created | Self::Unarchived | Self::Edited | Self::Renamed | Self::Publicized => {
                Some(true)
            }
            Self::Deleted | Self::Archived | Self::Privatized => Some(false),
            Self::Transferred => None,
        }
        // TODO extra logic with deletion
    }
}

#[derive(Deserialize)]
pub struct Installation {
    pub id: u64,
}

#[derive(Deserialize)]
pub struct Repo {
    pub id: u64,
    pub full_name: String,
}
