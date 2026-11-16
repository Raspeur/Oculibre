package org.oculibre

import android.app.Activity
import android.app.AlertDialog
import android.content.Context

/**
 * "Pair with <computer>?": the one confirmation of a pairing offer, whether it came from the in-app
 * scanner or from an `oculibre://pair` link (camera app, USB). Nothing is saved before the user
 * confirms, after comparing the code with the one the computer shows.
 */
object PairingPrompt {

    /**
     * Shows the dialog. [onDone] gets whether the phone is now paired, once the dialog is gone
     * (paired, cancelled, or dismissed with Back).
     */
    fun confirm(activity: Activity, offer: PairingOffer, onDone: (paired: Boolean) -> Unit) {
        // The computer's address on this phone's network (it may also have VPN or container ones).
        val address = Networks.rank(offer.addresses, Networks.local()).first()
        var paired = false
        AlertDialog.Builder(activity)
            .setTitle(activity.getString(R.string.pair_title, offer.name))
            .setMessage(activity.getString(R.string.pair_text, offer.shortFingerprint, address))
            .setPositiveButton(R.string.pair_action) { _, _ ->
                save(activity, offer, address)
                paired = true
            }
            .setNegativeButton(R.string.cancel, null)
            .setOnDismissListener { onDone(paired) }
            .show()
    }

    private fun save(context: Context, offer: PairingOffer, address: String) {
        PairingStore.save(context, offer)
        context.getSharedPreferences("oculibre", Context.MODE_PRIVATE).edit()
            .putString(MainActivity.PREF_HOST, address).apply()
    }
}
