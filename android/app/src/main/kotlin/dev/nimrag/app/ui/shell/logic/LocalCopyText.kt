package dev.nimrag.app.ui.shell.logic

/**
 * What the banner above the phone's own copy says: "This phone's copy", plus how many changes
 * made on it the server has not got yet. Pure, so the choice is tested; the words live in
 * `strings.xml` (the plural there gives "1 change" and "N changes").
 */
sealed interface LocalCopyText {
    data object Plain : LocalCopyText

    data class Waiting(val count: Int) : LocalCopyText

    companion object {
        fun of(pendingEdits: Int): LocalCopyText = if (pendingEdits > 0) Waiting(pendingEdits) else Plain
    }
}
