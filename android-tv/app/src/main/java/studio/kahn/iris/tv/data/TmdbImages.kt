package studio.kahn.iris.tv.data

// TMDB image URL helpers. The API returns bare TMDB paths (`/abc.jpg`) on the
// generated DTOs (`posterPath` / `backdropPath`); these prepend the CDN host +
// a size bucket. Hand-written view-layer helpers — not part of the generated
// contract.

/** Poster sizes: w92, w154, w185, w342, w500, w780, original. w342 for cards. */
fun tmdbPosterUrl(path: String?, size: String = "w342"): String? =
    path?.let { "https://image.tmdb.org/t/p/$size$it" }

/** Backdrop sizes: w300, w780, w1280, original. We use w780 for shelf cards. */
fun tmdbBackdropUrl(path: String?, size: String = "w780"): String? =
    path?.let { "https://image.tmdb.org/t/p/$size$it" }
