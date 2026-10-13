// SPDX-License-Identifier: Apache-2.0
// Copyright (c) 2026 MoleSignal Authors

//! 报表以谁的身份运行，给人看的那一面：这位用户的显示名和邮箱。
//!
//! 能排定报表的人不一定能读成员目录（`org.members.read`），被移出组织的用户也不在目录里。
//! 所以名字随报表一起返回：直接读用户记录，不看成员关系。

use std::collections::{BTreeSet, HashMap};

use serde::Serialize;

use crate::{
    domain::iam::{User, UserRepository},
    shared::{Error, Result, ids::Id},
};

/// 报表运行身份的显示信息。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RunAsProfile {
    pub id: Id,
    pub display_name: String,
    pub email: String,
}

/// 这些用户的显示信息，按用户 id 索引。
///
/// 用户记录已经不存在的 id 不在结果里：报表还记着这个 id，调用方把它当作只剩 id 的身份。
pub async fn run_as_profiles(
    users: &dyn UserRepository,
    user_ids: impl IntoIterator<Item = &Id>,
) -> Result<HashMap<Id, RunAsProfile>> {
    let ids = user_ids
        .into_iter()
        .map(Id::as_str)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(Id::from_string)
        .collect::<Vec<_>>();

    if ids.is_empty() {
        return Ok(HashMap::new());
    }

    let found = match users.get_many(&ids).await {
        Ok(found) => found,
        // 批量读取的默认实现逐个读，缺一个就整体失败。一位被删除的用户不该让报表读不出来。
        Err(Error::NotFound(_)) => existing(users, &ids).await?,
        Err(error) => return Err(error),
    };

    Ok(found
        .into_iter()
        .map(|user| {
            let profile = RunAsProfile {
                id: user.id.clone(),
                display_name: user.display_name,
                email: user.email,
            };

            (user.id, profile)
        })
        .collect())
}

/// 逐个读取，跳过已经不存在的用户。
async fn existing(users: &dyn UserRepository, ids: &[Id]) -> Result<Vec<User>> {
    let mut found = Vec::with_capacity(ids.len());

    for id in ids {
        match users.get(id).await {
            Ok(user) => found.push(user),
            Err(Error::NotFound(_)) => {}
            Err(error) => return Err(error),
        }
    }

    Ok(found)
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use async_trait::async_trait;

    use super::*;
    use crate::{domain::iam::UserStatus, shared::time::TimestampMicros};

    /// 批量读取遇到不存在的 id 时，适配器的两种行为。两种都要能用。
    #[derive(Clone, Copy)]
    enum Missing {
        /// Postgres 适配器：只返回存在的用户。
        LeftOut,
        /// trait 的默认实现：逐个读，缺一个就整体 `NotFound`。
        Fails,
    }

    /// 有 Ada 和 Bo 两位用户的目录，记下每次读取问了哪些 id。
    struct Users {
        missing: Missing,
        /// 用户表读不了：与某位用户不存在是两回事。
        unreachable: bool,
        asked: Mutex<Vec<Vec<String>>>,
    }

    impl Users {
        fn new(missing: Missing) -> Self {
            Self {
                missing,
                unreachable: false,
                asked: Mutex::new(Vec::new()),
            }
        }

        fn asked(&self) -> Vec<Vec<String>> {
            self.asked.lock().unwrap().clone()
        }

        fn find(&self, id: &Id) -> Result<User> {
            if self.unreachable {
                return Err(Error::internal("users table unreachable"));
            }

            [
                user("u-ada", "Ada", "ada@example.com"),
                // 没有填显示名的用户。
                user("u-bo", "", "bo@example.com"),
            ]
            .into_iter()
            .find(|user| &user.id == id)
            .ok_or_else(|| Error::not_found("user"))
        }
    }

    fn user(id: &str, display_name: &str, email: &str) -> User {
        User {
            id: Id::from_string(id),
            email: email.into(),
            display_name: display_name.into(),
            avatar_url: None,
            bio: String::new(),
            password_hash: "argon2-hash".into(),
            disabled: false,
            status: UserStatus::Active,
            created_at: TimestampMicros(0),
        }
    }

    fn unused<T>() -> Result<T> {
        Err(Error::internal("not used by run_as_profiles"))
    }

    #[async_trait]
    impl UserRepository for Users {
        async fn get(&self, id: &Id) -> Result<User> {
            self.asked.lock().unwrap().push(vec![id.0.clone()]);

            self.find(id)
        }

        async fn get_many(&self, ids: &[Id]) -> Result<Vec<User>> {
            self.asked
                .lock()
                .unwrap()
                .push(ids.iter().map(|id| id.0.clone()).collect());

            let mut found = Vec::new();

            for id in ids {
                match (self.find(id), self.missing) {
                    (Ok(user), _) => found.push(user),
                    (Err(Error::NotFound(_)), Missing::LeftOut) => {}
                    (Err(error), _) => return Err(error),
                }
            }

            Ok(found)
        }

        async fn create(&self, _: User) -> Result<User> {
            unused()
        }

        async fn get_by_email(&self, _: &str) -> Result<User> {
            unused()
        }

        async fn update(&self, _: User) -> Result<User> {
            unused()
        }

        async fn delete(&self, _: &Id) -> Result<()> {
            unused()
        }

        async fn count(&self) -> Result<u64> {
            unused()
        }

        async fn list(&self) -> Result<Vec<User>> {
            unused()
        }

        async fn set_status(&self, _: &Id, _: UserStatus) -> Result<()> {
            unused()
        }
    }

    fn ids(values: &[&str]) -> Vec<Id> {
        values.iter().copied().map(Id::from_string).collect()
    }

    #[tokio::test]
    async fn a_user_is_returned_with_the_name_and_email_people_know_them_by() {
        let users = Users::new(Missing::LeftOut);

        let profiles = run_as_profiles(&users, &ids(&["u-ada", "u-bo"]))
            .await
            .unwrap();

        assert_eq!(
            profiles.get(&Id::from_string("u-ada")),
            Some(&RunAsProfile {
                id: Id::from_string("u-ada"),
                display_name: "Ada".into(),
                email: "ada@example.com".into(),
            })
        );
        // 空的显示名照原样返回，由界面退到邮箱。
        assert_eq!(
            profiles.get(&Id::from_string("u-bo")),
            Some(&RunAsProfile {
                id: Id::from_string("u-bo"),
                display_name: String::new(),
                email: "bo@example.com".into(),
            })
        );
    }

    #[tokio::test]
    async fn only_what_is_shown_leaves_the_user_record() {
        let users = Users::new(Missing::LeftOut);

        let profiles = run_as_profiles(&users, &ids(&["u-ada"])).await.unwrap();

        // 没有密码哈希、头像、状态这些不该随报表出去的字段。
        assert_eq!(
            serde_json::to_value(&profiles[&Id::from_string("u-ada")]).unwrap(),
            serde_json::json!({
                "id": "u-ada",
                "display_name": "Ada",
                "email": "ada@example.com",
            })
        );
    }

    #[tokio::test]
    async fn many_reports_of_the_same_users_are_one_lookup() {
        let users = Users::new(Missing::LeftOut);

        run_as_profiles(&users, &ids(&["u-bo", "u-ada", "u-bo", "u-ada"]))
            .await
            .unwrap();

        assert_eq!(
            users.asked(),
            vec![vec!["u-ada".to_owned(), "u-bo".to_owned()]]
        );
    }

    #[tokio::test]
    async fn reports_without_a_user_ask_nothing() {
        let users = Users::new(Missing::LeftOut);

        let profiles = run_as_profiles(&users, &ids(&[])).await.unwrap();

        assert!(profiles.is_empty());
        assert!(users.asked().is_empty());
    }

    #[tokio::test]
    async fn a_deleted_user_is_left_out_and_the_others_are_still_found() {
        for missing in [Missing::LeftOut, Missing::Fails] {
            let users = Users::new(missing);

            let profiles = run_as_profiles(&users, &ids(&["u-ada", "u-gone"]))
                .await
                .unwrap();

            assert_eq!(profiles.len(), 1);
            assert_eq!(profiles[&Id::from_string("u-ada")].email, "ada@example.com");
            assert!(!profiles.contains_key(&Id::from_string("u-gone")));
        }
    }

    #[tokio::test]
    async fn a_user_table_that_cannot_be_read_is_an_error_not_a_missing_user() {
        let users = Users {
            unreachable: true,
            ..Users::new(Missing::LeftOut)
        };

        let error = run_as_profiles(&users, &ids(&["u-ada"])).await.unwrap_err();

        assert!(matches!(error, Error::Internal(_)), "got {error:?}");
    }
}
