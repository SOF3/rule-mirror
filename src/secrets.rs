// rule-mirror
// Copyright (C) SOFe
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU Affero General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU Affero General Public License for more details.
//
// You should have received a copy of the GNU Affer General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

#[derive(serde::Deserialize, getset::Getters)]
#[get = "pub"]
pub struct Secrets {
    db: DbSecrets,
    discord: DiscordSecrets,
    webhook: WebhookSecrets,
}

#[derive(serde::Deserialize, getset::Getters)]
#[get = "pub"]
pub struct DbSecrets {
    url: String,
}

#[derive(serde::Deserialize, getset::Getters)]
#[get = "pub"]
pub struct DiscordSecrets {
    token: String,
}

#[derive(serde::Deserialize, getset::Getters)]
#[get = "pub"]
pub struct WebhookSecrets {
    secret: String,
}

impl Secrets {
    pub fn try_new() -> Result<Self, config::ConfigError> {
        let mut config = config::Config::new();
        config.merge(config::File::with_name("secrets"))?;
        config.merge(config::Environment::new())?;
        config.try_into()
    }
}
