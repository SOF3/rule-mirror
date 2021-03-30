# Database schema
This app uses a Redis database with the following keys:

- `seen`: set of repo IDs that are known to be tracked by the github app
- `repo:{repo id}`: set of `{random id}` values for mirror groups corresponding to the repo
- `mirror-group:{random id}:path`: a string in the format `branch/path-to/file-to-mirror.txt`
- `mirror-group:{random id}:channel`: channel ID of the mirror group
- `mirror-group:{random id}:messages`: list of discord message IDs corresponding to this group
- `mirror-group-rev:{message id}`: the random id of the mirror group owning the message id
