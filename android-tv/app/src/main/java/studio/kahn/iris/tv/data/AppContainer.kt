package studio.kahn.iris.tv.data

import android.content.Context
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
    fun apiFor(baseUrl: String): IrisApi
}

class DefaultAppContainer(context: Context) : AppContainer {
    override val sessionStore: SessionStore = SessionStore(context.applicationContext)
    override val okHttpClient: OkHttpClient = buildOkHttpClient(sessionStore)
    override val channels: ChannelsService = ChannelsService(context.applicationContext)

    private val json = Json {
        ignoreUnknownKeys = true
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
