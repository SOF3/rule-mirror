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

#![feature(proc_macro_hygiene, decl_macro)]

use std::sync::Arc;
use std::thread;

dirmod::all!();

fn main() {
    pretty_env_logger::init();

    let secrets = Secrets::try_new().expect("Failed to read config");
    let secrets = Arc::new(secrets);

    let pool = {
        let secrets = Arc::clone(&secrets);
        db::init(&secrets).expect("Failed to open database pool")
    };
    let pool = Arc::new(pool);

    {
        let secrets = Arc::clone(&secrets);
        let pool = Arc::clone(&pool);
        thread::spawn(move || {
            discord::init(&secrets, &pool).unwrap();
        });
    }

    panic!(webhook::init(secrets, pool));
}
