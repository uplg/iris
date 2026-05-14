package studio.kahn.iris.tv.data

import android.content.Context
import kotlinx.coroutines.CoroutineScope
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.SupervisorJob
import kotlinx.coroutines.flow.MutableStateFlow
import kotlinx.coroutines.flow.StateFlow
import kotlinx.coroutines.flow.asStateFlow
import kotlinx.serialization.ExperimentalSerializationApi
import kotlinx.serialization.json.Json
import okhttp3.MediaType.Companion.toMediaType
import okhttp3.OkHttpClient
import retrofit2.Retrofit
import retrofit2.converter.kotlinx.serialization.asConverterFactory

/**
 * Singletons (HTTP, JSON, persistence). Recreated whenever the Iris server
 * URL changes, since the Retrofit base URL is baked at construction time.
 */
interface AppContainer {
    val sessionStore: SessionStore
    val okHttpClient: OkHttpClient
    val channels: ChannelsService
    /**
     * Process-lifetime scope for fire-and-forget background work that
     * MUST outlive the composable that started it. Typical user:
     * `saveProgress` calls fired from a `DisposableEffect.onDispose`
     * when the user navigates back — `rememberCoroutineScope` would
     * cancel before the request leaves the device, dropping the user's
     * latest audio / subtitle picks. Use sparingly; the scope never
     * gets cancelled, so anything started here keeps a coroutine alive
     * until it completes naturally.
     */
    val applicationScope: CoroutineScope
    /**
     * Flips to `true` the first time the backend answers with `426
     * Upgrade Required`, signalling this APK is below the server's
     * `MIN_TV_VERSION`. The root composable observes it and shows a
     * full-screen "please update" overlay that blocks the rest of the
     * UI until the user installs a newer APK.
     */
    val clientOutdated: StateFlow<Boolean>
    fun apiFor(baseUrl: String): IrisApi
}

class DefaultAppContainer(context: Context) : AppContainer {
    override val sessionStore: SessionStore = SessionStore(context.applicationContext)
    private val outdatedFlag = MutableStateFlow(false)
    override val clientOutdated: StateFlow<Boolean> = outdatedFlag.asStateFlow()
    override val okHttpClient: OkHttpClient =
        buildOkHttpClient(sessionStore) { outdatedFlag.value = true }
    override val channels: ChannelsService = ChannelsService(context.applicationContext)
    override val applicationScope: CoroutineScope =
        CoroutineScope(SupervisorJob() + Dispatchers.IO)

    private val json = Json {
        // Backward-compat guarantees for shipping APKs in the wild:
        //  - `ignoreUnknownKeys`: backend can add fields freely (e.g.
        //    `description_format`) without breaking older clients.
        //  - `coerceInputValues`: enum variants added on the backend
        //    fall back to the Kotlin default instead of throwing
        //    (forward-compat for `DescriptionFormat` / future enums).
        //  - `explicitNulls = false`: optional fields omitted when null,
        //    keeping payloads compact and avoiding nullable churn.
        ignoreUnknownKeys = true
        coerceInputValues = true
        explicitNulls = false
    }

    @OptIn(ExperimentalSerializationApi::class)
    override fun apiFor(baseUrl: String): IrisApi =
        Retrofit.Builder()
            .baseUrl(normalize(baseUrl))
            .client(okHttpClient)
            .addConverterFactory(json.asConverterFactory("application/json".toMediaType()))
            .build()
            .create(IrisApi::class.java)

    private fun normalize(url: String): String =
        if (url.endsWith("/")) url else "$url/"
}
