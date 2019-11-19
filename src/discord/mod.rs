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

dirmod::all!();

use serenity::client::Client;

use crate::Secrets;
use crate::db::Pool;

pub fn init(secrets: &Secrets, _pool: &Pool) -> serenity::Result<()> {
    let mut client = Client::new(&secrets.discord().token(), Handler)?;
    client.start()
}
