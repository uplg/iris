package studio.kahn.iris.tv.data

import androidx.test.core.app.ApplicationProvider
import kotlinx.coroutines.flow.first
import kotlinx.coroutines.runBlocking
import okhttp3.OkHttpClient
import okhttp3.Protocol
import okhttp3.Request
import okhttp3.Response
import okhttp3.ResponseBody.Companion.toResponseBody
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.RobolectricTestRunner

/** Pairing seeds a session with no cookies for the poll's Set-Cookie to fill: a 401 meanwhile keeps it. */
@RunWith(RobolectricTestRunner::class)
class PairingSessionTest {
    @Test
    fun aStray401DuringPairingNeitherRefreshesNorDropsTheSeededSession() = runBlocking {
        val store = SessionStore(ApplicationProvider.getApplicationContext())
        val seeded = IrisSession("https://iris.example", email = "", isAdmin = false, cookies = emptyList())
        store.saveSession(seeded)
        fun unauthorized(request: Request) = Response.Builder()
            .request(request)
            .protocol(Protocol.HTTP_1_1)
            .code(401)
            .message("Unauthorized")
            .body("".toResponseBody())
            .build()
        var refreshes = 0
        val auth = IrisAuthenticator(store).apply {
            // A refresh with no refresh cookie answers 401, which reads as a dead session.
            bind(OkHttpClient.Builder().addInterceptor { chain -> refreshes++; unauthorized(chain.request()) }.build())
        }
        val unauthorized = unauthorized(Request.Builder().url("https://iris.example/api/torrents").build())

        assertNull(auth.authenticate(null, unauthorized))
        assertEquals(seeded, store.session.first())
        assertEquals(0, refreshes)
    }
}
