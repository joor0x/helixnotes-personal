package com.helixnotes.app

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent

/** Alarms going off and the snooze buttons of a reminder notification (not exported). */
class ReminderReceiver : BroadcastReceiver() {
  override fun onReceive(context: Context, intent: Intent) {
    val id = intent.getStringExtra(Reminders.EXTRA_ID) ?: return
    when (intent.action) {
      Reminders.ACTION_FIRE -> Reminders.fire(context, id)
      Reminders.ACTION_SNOOZE -> Reminders.snooze(
        context, id, intent.getStringExtra(Reminders.EXTRA_SNOOZE) ?: Reminders.SNOOZE_10_MIN
      )
    }
  }
}

/**
 * Android drops alarms on reboot and on app updates, and local reminder times move with the
 * clock and the time zone. All it ever does is reschedule what is already stored.
 */
class ReminderRestoreReceiver : BroadcastReceiver() {
  override fun onReceive(context: Context, intent: Intent) {
    Reminders.scheduleAll(context)
  }
}
