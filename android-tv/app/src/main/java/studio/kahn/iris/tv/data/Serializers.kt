package studio.kahn.iris.tv.data

import kotlinx.serialization.KSerializer
import kotlinx.serialization.descriptors.PrimitiveKind
import kotlinx.serialization.descriptors.PrimitiveSerialDescriptor
import kotlinx.serialization.descriptors.SerialDescriptor
import kotlinx.serialization.encoding.Decoder
import kotlinx.serialization.encoding.Encoder
import kotlinx.serialization.modules.SerializersModule
import java.time.OffsetDateTime
import java.util.UUID

// The OpenAPI-generated DTOs type `format: date-time` as java.time.OffsetDateTime
// and `format: uuid` as java.util.UUID, both `@Contextual`. We generate models
// only, so openapi-generator's infrastructure (which would register these) isn't
// emitted — we supply the serializers here and wire them onto the `Json` in
// AppContainer. (The `@Contextual` enums need nothing: kotlinx falls back to
// their own `@Serializable` serializer; plain Java classes have no such fallback,
// hence the runtime "Serializer for class … not found" without this.)
// Both round-trip through the wire's ISO-8601 / canonical-UUID strings.

private object OffsetDateTimeSerializer : KSerializer<OffsetDateTime> {
    override val descriptor: SerialDescriptor =
        PrimitiveSerialDescriptor("java.time.OffsetDateTime", PrimitiveKind.STRING)

    override fun serialize(encoder: Encoder, value: OffsetDateTime) =
        encoder.encodeString(value.toString())

    override fun deserialize(decoder: Decoder): OffsetDateTime =
        OffsetDateTime.parse(decoder.decodeString())
}

private object UuidSerializer : KSerializer<UUID> {
    override val descriptor: SerialDescriptor =
        PrimitiveSerialDescriptor("java.util.UUID", PrimitiveKind.STRING)

    override fun serialize(encoder: Encoder, value: UUID) =
        encoder.encodeString(value.toString())

    override fun deserialize(decoder: Decoder): UUID =
        UUID.fromString(decoder.decodeString())
}

/** Contextual serializers for the `@Contextual` Java types in the generated DTOs. */
val irisSerializersModule: SerializersModule = SerializersModule {
    contextual(OffsetDateTime::class, OffsetDateTimeSerializer)
    contextual(UUID::class, UuidSerializer)
}
