package studio.kahn.iris.tv.data

import kotlinx.serialization.Serializable
import kotlinx.serialization.json.Json
import retrofit2.HttpException

/**
 * Error envelope every Iris endpoint answers with on 4xx/5xx:
 * `{"error": "<code>", "message": "<human text>"}`. Codes the TV client
 * branches on: `dead_torrent`, `archive_only`, `duplicate_in_library`.
 */
@Serializable
data class ApiErrorEnvelope(
    val error: String? = null,
    val message: String? = null,
)

private val envelopeJson = Json {
    ignoreUnknownKeys = true
    coerceInputValues = true
}

/** Parsed Iris error envelope, or null when the body isn't one. */
fun HttpException.irisError(): ApiErrorEnvelope? = runCatching {
    response()?.errorBody()?.string()?.let {
        envelopeJson.decodeFromString<ApiErrorEnvelope>(it)
    }
}.getOrNull()
