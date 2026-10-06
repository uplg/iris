package studio.kahn.iris.tv.data

/** The library as torrents (`/api/library?view=torrents`). */
suspend fun IrisApi.libraryTorrents(): LibraryResponseTorrents =
    (library("torrents") as? LibraryResponse.TorrentsWrapper)?.value ?: error(LIBRARY_SHAPE)

/** The library as titles (`/api/library?view=collections`). */
suspend fun IrisApi.libraryCollections(): LibraryResponseCollections =
    (library("collections") as? LibraryResponse.CollectionsWrapper)?.value ?: error(LIBRARY_SHAPE)

private const val LIBRARY_SHAPE = "The server answered the library in a form this app does not read. Update Iris TV from Settings."
