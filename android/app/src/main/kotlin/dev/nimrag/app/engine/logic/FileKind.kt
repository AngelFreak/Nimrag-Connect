package dev.nimrag.app.engine.logic

/**
 * The label of a stored file in the UI: "FIT 128/4 activity". Data type 128 is the FIT
 * family; names are the ones in docs/protocol/filetypes.csv (and the plan's wording for
 * 32 and 68), shown only where they are known.
 */
object FileKind {
    private val fitNames = mapOf(
        4 to "activity",
        15 to "monitoring_a",
        28 to "monitoring_daily",
        32 to "monitoring",
        41 to "changelog",
        44 to "metrics",
        49 to "sleep",
        52 to "user_behavior_log",
        57 to "sports backup",
        58 to "device",
        68 to "HRV status",
    )

    const val FIT_DATA_TYPE = 128

    fun label(dataType: Int, subType: Int): String {
        if (dataType == FIT_DATA_TYPE) {
            val name = fitNames[subType]
            return if (name == null) "FIT $dataType/$subType" else "FIT $dataType/$subType $name"
        }
        if (dataType == 0 && subType == 0) return "directory 0/0"
        return "type $dataType/$subType"
    }
}
