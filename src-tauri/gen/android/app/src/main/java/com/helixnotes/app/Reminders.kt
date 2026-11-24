package com.helixnotes.app

import android.app.AlarmManager
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import java.text.SimpleDateFormat
import java.util.Calendar
import java.util.Locale
import org.json.JSONArray
import org.json.JSONObject

/**
 * Task reminders as native alarms, so they fire while HelixNotes is closed.
 *
 * The web app hands over the open reminders of the active vault ([sync]); they are kept in
 * SharedPreferences so alarms can be restored after a reboot without starting the web view.
 * Snoozing happens entirely here: it never rewrites the note, so a closed app cannot race
 * the editor or a vault sync.
 */
object Reminders {
  const val ACTION_FIRE = "com.helixnotes.app.REMINDER_FIRE"
  const val ACTION_SNOOZE = "com.helixnotes.app.REMINDER_SNOOZE"
  const val ACTION_OPEN = "com.helixnotes.app.REMINDER_OPEN"
  const val EXTRA_ID = "reminder_id"
  const val EXTRA_SNOOZE = "reminder_snooze"
  const val EXTRA_NOTE_PATH = "reminder_note_path"

  const val SNOOZE_10_MIN = "10m"
  const val SNOOZE_1_HOUR = "1h"
  const val SNOOZE_TOMORROW = "tomorrow"

  private const val PREFS = "helix_reminders"
  private const val KEY_PAYLOAD = "payload"
  private const val KEY_FIRED = "fired"
  private const val KEY_SNOOZED = "snoozed"
  private const val CHANNEL = "reminders"
  private const val SCHEME = "helixnotes-reminder"
  private const val TOMORROW_HOUR = 9

  /** A reminder missed while the phone was off still fires if it is this recent. */
  private const val GRACE_MS = 24L * 60 * 60 * 1000
  /** Fired reminders that left the list are remembered this long, so they fire once. */
  private const val RETENTION_MS = 7L * 24 * 60 * 60 * 1000

  private class Reminder(
    val id: String,
    /** Local wall-clock time, `yyyy-MM-ddTHH:mm`; resolved with the current time zone. */
    val at: String,
    val title: String,
    val body: String,
    val path: String
  )

  private fun prefs(context: Context) =
    context.applicationContext.getSharedPreferences(PREFS, Context.MODE_PRIVATE)

  private fun payload(context: Context): JSONObject =
    try {
      JSONObject(prefs(context).getString(KEY_PAYLOAD, null) ?: "{}")
    } catch (_: Exception) {
      JSONObject()
    }

  private fun reminders(payload: JSONObject): List<Reminder> {
    val list = payload.optJSONArray("reminders") ?: JSONArray()
    return (0 until list.length()).mapNotNull { index ->
      val item = list.optJSONObject(index) ?: return@mapNotNull null
      val id = item.optString("id")
      val at = item.optString("at")
      if (id.isEmpty() || at.isEmpty()) return@mapNotNull null
      Reminder(id, at, item.optString("title"), item.optString("body"), item.optString("path"))
    }
  }

  private fun label(payload: JSONObject, key: String, fallback: String): String =
    payload.optJSONObject("labels")?.optString(key)?.takeIf { it.isNotEmpty() } ?: fallback

  private fun times(context: Context, key: String): MutableMap<String, Long> {
    val result = mutableMapOf<String, Long>()
    try {
      val json = JSONObject(prefs(context).getString(key, null) ?: "{}")
      for (id in json.keys()) result[id] = json.getLong(id)
    } catch (_: Exception) {
    }
    return result
  }

  private fun saveTimes(context: Context, key: String, values: Map<String, Long>) {
    val json = JSONObject()
    for ((id, time) in values) json.put(id, time)
    prefs(context).edit().putString(key, json.toString()).apply()
  }

  private fun parseLocal(at: String): Long? =
    try {
      SimpleDateFormat("yyyy-MM-dd'T'HH:mm", Locale.US).apply { isLenient = false }.parse(at)?.time
    } catch (_: Exception) {
      null
    }

  private fun uri(id: String, kind: String): Uri =
    Uri.Builder().scheme(SCHEME).authority(kind).appendPath(id).build()

  private fun firePendingIntent(context: Context, id: String): PendingIntent {
    val intent = Intent(context, ReminderReceiver::class.java)
      .setAction(ACTION_FIRE)
      .setData(uri(id, "fire"))
      .putExtra(EXTRA_ID, id)
    return PendingIntent.getBroadcast(
      context, 0, intent, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
    )
  }

  private fun setAlarm(context: Context, id: String, at: Long) {
    val alarms = context.getSystemService(Context.ALARM_SERVICE) as AlarmManager
    val intent = firePendingIntent(context, id)
    val exact = Build.VERSION.SDK_INT < Build.VERSION_CODES.S || alarms.canScheduleExactAlarms()
    try {
      if (exact) alarms.setExactAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, at, intent)
      else alarms.setAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, at, intent)
    } catch (_: SecurityException) {
      alarms.setAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, at, intent)
    }
  }

  private fun cancelAlarm(context: Context, id: String) {
    val alarms = context.getSystemService(Context.ALARM_SERVICE) as AlarmManager
    alarms.cancel(firePendingIntent(context, id))
  }

  private fun notificationId(id: String): Int = id.hashCode()

  /** Replaces the known reminders with `json` (`{"reminders": [...], "labels": {...}}`). */
  @Synchronized
  fun sync(context: Context, json: String): Int {
    val next = try {
      JSONObject(json)
    } catch (_: Exception) {
      return 0
    }
    val kept = reminders(next).map { it.id }.toSet()
    val snoozed = times(context, KEY_SNOOZED)
    val notifications = NotificationManagerCompat.from(context)
    for (old in reminders(payload(context))) {
      if (old.id in kept) continue
      // Completed, edited or deleted: nothing left to remind about.
      cancelAlarm(context, old.id)
      notifications.cancel(notificationId(old.id))
      snoozed.remove(old.id)
    }
    saveTimes(context, KEY_SNOOZED, snoozed)
    prefs(context).edit().putString(KEY_PAYLOAD, next.toString()).apply()
    scheduleAll(context)
    return kept.size
  }

  /** (Re)creates every pending alarm: after a sync, a reboot, an update or a clock change. */
  @Synchronized
  fun scheduleAll(context: Context) {
    val now = System.currentTimeMillis()
    val list = reminders(payload(context))
    val fired = times(context, KEY_FIRED)
    val snoozed = times(context, KEY_SNOOZED)
    for (reminder in list) {
      val snoozedUntil = snoozed[reminder.id]
      if (snoozedUntil != null) {
        setAlarm(context, reminder.id, maxOf(snoozedUntil, now))
        continue
      }
      if (reminder.id in fired) continue
      val at = parseLocal(reminder.at) ?: continue
      if (at < now - GRACE_MS) {
        fired[reminder.id] = now
        continue
      }
      // A time in the recent past fires right away.
      setAlarm(context, reminder.id, at)
    }
    val known = list.map { it.id }.toSet()
    fired.entries.removeAll { it.key !in known && it.value < now - RETENTION_MS }
    saveTimes(context, KEY_FIRED, fired)
  }

  @Synchronized
  fun fire(context: Context, id: String) {
    val stored = payload(context)
    val reminder = reminders(stored).firstOrNull { it.id == id } ?: return
    val now = System.currentTimeMillis()
    val snoozed = times(context, KEY_SNOOZED)
    if (snoozed.remove(id) != null) saveTimes(context, KEY_SNOOZED, snoozed)
    val fired = times(context, KEY_FIRED)
    fired[id] = now
    saveTimes(context, KEY_FIRED, fired)
    show(context, stored, reminder)
  }

  @Synchronized
  fun snooze(context: Context, id: String, kind: String) {
    NotificationManagerCompat.from(context).cancel(notificationId(id))
    if (reminders(payload(context)).none { it.id == id }) return
    val until = snoozeTime(kind, System.currentTimeMillis())
    val snoozed = times(context, KEY_SNOOZED)
    snoozed[id] = until
    saveTimes(context, KEY_SNOOZED, snoozed)
    setAlarm(context, id, until)
  }

  private fun snoozeTime(kind: String, now: Long): Long = when (kind) {
    SNOOZE_1_HOUR -> now + 60L * 60 * 1000
    SNOOZE_TOMORROW -> Calendar.getInstance().apply {
      timeInMillis = now
      add(Calendar.DAY_OF_YEAR, 1)
      set(Calendar.HOUR_OF_DAY, TOMORROW_HOUR)
      set(Calendar.MINUTE, 0)
      set(Calendar.SECOND, 0)
      set(Calendar.MILLISECOND, 0)
    }.timeInMillis
    else -> now + 10L * 60 * 1000
  }

  private fun snoozeAction(
    context: Context, payload: JSONObject, id: String, kind: String, labelKey: String, fallback: String
  ): NotificationCompat.Action {
    val intent = Intent(context, ReminderReceiver::class.java)
      .setAction(ACTION_SNOOZE)
      .setData(uri(id, kind))
      .putExtra(EXTRA_ID, id)
      .putExtra(EXTRA_SNOOZE, kind)
    val pending = PendingIntent.getBroadcast(
      context, 0, intent, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
    )
    return NotificationCompat.Action.Builder(0, label(payload, labelKey, fallback), pending).build()
  }

  private fun show(context: Context, payload: JSONObject, reminder: Reminder) {
    val notifications = NotificationManagerCompat.from(context)
    if (!notifications.areNotificationsEnabled()) return
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
      val manager = context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
      manager.createNotificationChannel(
        NotificationChannel(
          CHANNEL, label(payload, "channel", "Reminders"), NotificationManager.IMPORTANCE_HIGH
        )
      )
    }
    val open = Intent(context, MainActivity::class.java)
      .setAction(ACTION_OPEN)
      .setData(uri(reminder.id, "open"))
      .putExtra(EXTRA_NOTE_PATH, reminder.path)
      .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
    val content = PendingIntent.getActivity(
      context, 0, open, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE
    )
    val id = reminder.id
    val notification = NotificationCompat.Builder(context, CHANNEL)
      .setSmallIcon(R.drawable.ic_stat_reminder)
      .setContentTitle(reminder.title)
      .setContentText(reminder.body)
      .setStyle(NotificationCompat.BigTextStyle().bigText(reminder.body))
      .setCategory(NotificationCompat.CATEGORY_REMINDER)
      .setPriority(NotificationCompat.PRIORITY_HIGH)
      .setDefaults(NotificationCompat.DEFAULT_ALL)
      .setAutoCancel(true)
      .setContentIntent(content)
      .addAction(snoozeAction(context, payload, id, SNOOZE_10_MIN, "snooze10", "10 min"))
      .addAction(snoozeAction(context, payload, id, SNOOZE_1_HOUR, "snooze60", "1 hour"))
      .addAction(snoozeAction(context, payload, id, SNOOZE_TOMORROW, "snoozeTomorrow", "Tomorrow"))
      .build()
    try {
      notifications.notify(notificationId(id), notification)
    } catch (_: SecurityException) {
      // The notification permission was revoked between the check and the post.
    }
  }
}
