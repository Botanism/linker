//! # Error
//!
//! Because all endpoints make use of the underlying database they can all fail due to it.
//! As such errors caused by the database are mostly undocumented. Instead endpoints will only provide
//! an `Error` section if APi-specific errors can occur.

#[cfg(test)]
mod tests;

use db_adapter::{
    establish_connection,
    guild::{GuildConfig, GuildConfigBuilder, GuildConfigError, Privilege},
    slap::{GuildSlapRecord, MemberSlapRecord, SlapReport},
    AdapterError, PgPool,
};
use dotenv::dotenv;
use rocket::{
    form::{Form, FromForm},
    get,
    http::ContentType,
    http::Status,
    post,
    request::Request,
    response::{self, Responder, Response},
    routes,
    serde::json::Json,
    State,
};
use serenity::model::id::{GuildId, RoleId, UserId};
use std::{io::Cursor, u64};
use thiserror;
use tokio_stream::StreamExt;

type Pool = State<PgPool>;

/// Wrapper around [`AdapterError`]
#[derive(Debug, thiserror::Error)]
enum ApiError {
    #[error("We couldn't process your request: {reason}. Error: {source}")]
    AdapterError {
        status: Status,
        reason: String,
        #[source]
        source: AdapterError,
    },
    #[error("expected on of: `admin`, `event` or `manager` found {0}")]
    UnrecognizedPrivilege(String),
}

impl<'a> From<AdapterError> for ApiError {
    fn from(err: AdapterError) -> Self {
        let (status, reason) = match &err {
            AdapterError::SqlxError(_) => (
                Status::InternalServerError,
                "sqlx driver failed to query the database",
            ),
            AdapterError::GuildError(guild_error) => match guild_error {
                GuildConfigError::AlreadyExists(_id) => {
                    (Status::BadRequest, "guild already exists")
                }
                _ => todo!(),
            },
        };

        ApiError::AdapterError {
            status,
            reason: reason.to_string(),
            source: err,
        }
    }
}

impl<'r, 'o: 'r> Responder<'r, 'o> for ApiError {
    fn respond_to(self, _: &'r Request<'_>) -> response::Result<'o> {
        let mut response = Response::build();
        response.header(ContentType::Plain);
        match self {
            ApiError::AdapterError { status, reason, .. } => response
                .status(status)
                .sized_body(reason.len(), Cursor::new(reason)),
            ApiError::UnrecognizedPrivilege(_) => response.status(Status::BadRequest),
        };

        response.ok()
    }
}

type ApiResult<T> = Result<T, ApiError>;

#[rocket::main]
async fn main() {
    dotenv().ok();
    rocket::build()
        //TODO: try and optimise this since every call only requires &PgPool (ie: references)
        .manage(establish_connection().await)
        .mount(
            "/",
            routes![
                gsr_len,
                gsr_slaps,
                gsr_offenders,
                new_slap,
                msr_len,
                msr_slaps,
                guild_admin_chan,
                guild_advertise,
                guild_exists,
                guild_goodbye_message,
                guild_has_privileges,
                guild_privileges_for,
                guild_roles_with,
                guild_welcome_message,
                guild_have_privilege,
                guild_new,
                guild_set_admin_chan,
                guild_set_advertise,
                guild_set_welcome_message,
                guild_set_goodbye_message
            ],
        )
        .launch()
        .await
        .unwrap();
}

/// `GET` up to `number` [`SlapReport`] from the guild.
///
///Currently there's no way to stream all [`SlapReport`] from a guild so this is often used alongside
///[`gsr_len()`]. Otherwise you may provide a very big `number` *should* give them all.
///
/// # Errors
///
/// Aside from failures from the underlying database, the request will fail if `number` is greater than
/// either 2^32 or 2^64 depending on the platform.
#[get("/slaps/<guild>/reports?<number>")]
async fn gsr_slaps(guild: u64, number: usize, pool: &Pool) -> ApiResult<Json<Vec<SlapReport>>> {
    Ok(Json(
        GuildSlapRecord::from(GuildId(guild))
            .slaps(pool.inner())
            .take(number)
            .collect::<Result<Vec<SlapReport>, AdapterError>>()
            .await?,
    ))
}

/// `GET` up to `number` [`UserId`] (`u64`) who were slapped in the guild.
///
///Currently there's no way to stream all [`UserId`] from a guild so this is often used alongside
///[`gsr_offender_len`]. Otherwise you may provide a very big `number` *should* give them all.
///
/// # Errors
///
/// Aside from failures from the underlying database, the request will fail if `number` is greater than
/// either 2^32 or 2^64 depending on the platform.
#[get("/slaps/<guild>/offenders?<number>")]
async fn gsr_offenders(guild: u64, number: usize, pool: &Pool) -> ApiResult<Json<Vec<u64>>> {
    Ok(Json(
        GuildSlapRecord::from(GuildId(guild))
            .offenders(pool.inner())
            .take(number)
            .map(|res| res.map(|msr| msr.1 .0))
            .collect::<Result<Vec<u64>, AdapterError>>()
            .await?,
    ))
}

/// `GET` the number of slaps in the guild
#[get("/slaps/<guild>/len")]
async fn gsr_len(pool: &Pool, guild: u64) -> ApiResult<Json<usize>> {
    Ok(Json(
        GuildSlapRecord::from(GuildId(guild))
            .len(pool.inner())
            .await?,
    ))
}

#[derive(Debug, FromForm)]
struct SlapForm {
    guild: u64,
    sentence: u64,
    offender: u64,
    enforcer: Option<u64>,
    reason: Option<String>,
}

#[post("/slaps/new", data = "<slap>")]
async fn new_slap(pool: &Pool, slap: Form<SlapForm>) -> ApiResult<Json<SlapReport>> {
    let gsr = GuildSlapRecord(slap.guild.into());
    Ok(Json(
        gsr.new_slap(
            pool.inner(),
            slap.sentence.into(),
            slap.offender.into(),
            slap.enforcer.into(),
            slap.reason.as_ref(),
        )
        .await?,
    ))
}

/// `GET` the number of slaps in the guild for `member` ([`UserId`])
#[get("/slaps/<guild>/<member>/len")]
async fn msr_len(pool: &Pool, guild: u64, member: u64) -> ApiResult<Json<usize>> {
    Ok(Json(
        MemberSlapRecord::from((GuildId(guild), UserId(member)))
            .len(pool.inner())
            .await?,
    ))
}

#[get("/slaps/<guild>/<member>/reports?<number>")]
async fn msr_slaps(
    guild: u64,
    member: u64,
    number: usize,
    pool: &Pool,
) -> ApiResult<Json<Vec<SlapReport>>> {
    Ok(Json(
        MemberSlapRecord::from((GuildId(guild), UserId(member)))
            .slaps(pool.inner())
            .take(number)
            .collect::<Result<Vec<SlapReport>, AdapterError>>()
            .await?,
    ))
}

#[get("/guild/<guild>/exists")]
async fn guild_exists(pool: &Pool, guild: u64) -> ApiResult<Json<bool>> {
    Ok(Json(GuildConfig(guild.into()).exists(pool.inner()).await?))
}

#[get("/guild/<guild>/admin_channel")]
async fn guild_admin_chan(pool: &Pool, guild: u64) -> ApiResult<Json<Option<u64>>> {
    Ok(Json(
        GuildConfig(guild.into())
            .get_admin_chan(pool.inner())
            .await?
            .map(|chan_id| chan_id.into()),
    ))
}

#[get("/guild/<guild>/advertise")]
async fn guild_advertise(pool: &Pool, guild: u64) -> ApiResult<Json<bool>> {
    Ok(Json(
        GuildConfig(guild.into())
            .get_advertise(pool.inner())
            .await?,
    ))
}

#[get("/guild/<guild>/goodbye_message")]
async fn guild_goodbye_message(pool: &Pool, guild: u64) -> ApiResult<Json<Option<String>>> {
    Ok(Json(
        GuildConfig(guild.into())
            .get_goodbye_message(pool.inner())
            .await?,
    ))
}

#[get("/guild/<guild>/welcome_message")]
async fn guild_welcome_message(pool: &Pool, guild: u64) -> ApiResult<Json<Option<String>>> {
    Ok(Json(
        GuildConfig(guild.into())
            .get_welcome_message(pool.inner())
            .await?,
    ))
}

#[get("/guild/<guild>/privileges/for_role/<role>")]
async fn guild_privileges_for(pool: &Pool, guild: u64, role: u64) -> ApiResult<Json<Vec<String>>> {
    let privs = GuildConfig(guild.into())
        .get_privileges_for(pool.inner(), role.into())
        .await?
        .iter()
        //consider finding a more optimized way to do this
        .map(|privilege| privilege.as_ref().into())
        .collect();
    Ok(Json(privs))
}

//TODO: good candiadate for a TryInto impl -> see db-adapter
fn str_to_priv(src: &str) -> ApiResult<Privilege> {
    Ok(match src {
        "admin" => Privilege::Admin,
        "manager" => Privilege::Manager,
        "event" => Privilege::Event,
        _ => return Err(ApiError::UnrecognizedPrivilege(src.into())),
    })
}

#[get("/guild/<guild>/privileges/roles_with/<privilege_str>")]
async fn guild_roles_with(
    pool: &Pool,
    guild: u64,
    privilege_str: &str,
) -> ApiResult<Json<Vec<u64>>> {
    Ok(Json(
        GuildConfig(guild.into())
            .get_roles_with(pool.inner(), str_to_priv(privilege_str)?)
            .await?
            .iter()
            .map(|role| u64::from(*role))
            .collect::<Vec<u64>>(),
    ))
}

#[get("/guild/<guild>/privileges/has/<role>?<privileges_str>")]
async fn guild_has_privileges(
    pool: &Pool,
    guild: u64,
    role: u64,
    privileges_str: Vec<String>,
) -> ApiResult<Json<bool>> {
    let mut privileges = Vec::with_capacity(privileges_str.len());
    for string in privileges_str {
        privileges.push(str_to_priv(string.as_str())?)
    }
    Ok(Json(
        GuildConfig(guild.into())
            .has_privileges(pool.inner(), role.into(), privileges.as_slice())
            .await?,
    ))
}

#[get("/guild/<guild>/privileges/have/<privilege_str>?<roles>")]
async fn guild_have_privilege(
    pool: &Pool,
    guild: u64,
    roles: Vec<u64>,
    privilege_str: &str,
) -> ApiResult<Json<bool>> {
    Ok(Json(
        GuildConfig(guild.into())
            .have_privilege(
                pool.inner(),
                roles
                    .iter()
                    .map(|int| RoleId(*int))
                    .collect::<Vec<RoleId>>()
                    .as_slice(),
                str_to_priv(privilege_str)?,
            )
            .await?,
    ))
}

#[derive(Debug, FromForm)]
struct NewGuildForm {
    id: u64,
    welcome_message: Option<String>,
    goodbye_message: Option<String>,
    advertise: bool,
}

#[post("/guild/new", data = "<config>")]
async fn guild_new<'a>(pool: &Pool, config: Form<NewGuildForm>) -> ApiResult<()> {
    //consider moving some of this code into an `TryFrom` impl and call `into_inner` instead
    let mut builder = GuildConfigBuilder::new(config.id.into());
    builder.advertise(config.advertise);
    if let Some(welcome) = &config.welcome_message {
        builder.welcome_message(welcome.as_str())?;
    }
    if let Some(goodbye) = &config.goodbye_message {
        builder.welcome_message(goodbye.as_str())?;
    }

    GuildConfig::new(pool.inner(), builder).await?;
    Ok(())
}

#[post("/guild/<guild>/admin_channel", data = "<chan>")]
async fn guild_set_admin_chan(pool: &Pool, guild: u64, chan: Form<Option<u64>>) -> ApiResult<()> {
    Ok(GuildConfig(guild.into())
        .set_admin_chan(pool.inner(), chan.into_inner().map(|int| int.into()))
        .await?)
}

#[post("/guild/<guild>/advertise", data = "<policy>")]
async fn guild_set_advertise(pool: &Pool, guild: u64, policy: Form<bool>) -> ApiResult<()> {
    Ok(GuildConfig(guild.into())
        .set_advertise(pool.inner(), policy.into_inner())
        .await?)
}

#[post("/guild/<guild>/welcome_message", data = "<message>")]
async fn guild_set_welcome_message(
    pool: &Pool,
    guild: u64,
    message: Form<Option<&str>>,
) -> ApiResult<()> {
    Ok(GuildConfig(guild.into())
        .set_welcome_message(pool.inner(), message.into_inner())
        .await?)
}

#[post("/guild/<guild>/goodbye_message", data = "<message>")]
async fn guild_set_goodbye_message(
    pool: &Pool,
    guild: u64,
    message: Form<Option<&str>>,
) -> ApiResult<()> {
    Ok(GuildConfig(guild.into())
        .set_goodbye_message(pool.inner(), message.into_inner())
        .await?)
}
