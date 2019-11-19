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

use std::sync::Arc;

use rocket::State;

use crate::Secrets;
use crate::db::Pool;

dirmod::all!();

pub fn init(secrets: Arc<Secrets>, pool: Arc<Pool>) -> rocket::error::LaunchError {
    rocket::ignite()
        .manage(secrets)
        .manage(pool)
        .mount("/", rocket::routes! [
            home,
            webhook,
        ])
        .launch()
}

#[rocket::get("/")]
fn home() -> &'static str{
    "Hello world!"
}

#[rocket::get("/github-webhook")]
fn webhook(secrets: State<Arc<Secrets>>) {
    let _secret = secrets.webhook().secret();
}
