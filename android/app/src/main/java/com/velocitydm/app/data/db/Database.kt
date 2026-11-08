package com.velocitydm.app.data.db

import android.content.Context
import androidx.room.Dao
import androidx.room.Database
import androidx.room.Delete
import androidx.room.Entity
import androidx.room.Insert
import androidx.room.PrimaryKey
import androidx.room.Query
import androidx.room.Room
import androidx.room.RoomDatabase
import androidx.room.Update
import kotlinx.coroutines.flow.Flow

// SPDX-License-Identifier: GPL-3.0-or-later

@Entity(tableName = "downloads")
data class DownloadEntity(
    @PrimaryKey(autoGenerate = true) val id: Long = 0,
    val url: String,
    val mirrorsJson: String = "[]",
    val filename: String,
    val dir: String,
    val total: Long = 0,
    val downloaded: Long = 0,
    val state: String = STATE_QUEUED,
    val createdAt: Long = System.currentTimeMillis(),
    val completedAt: Long = 0,
    val errorMsg: String? = null,
    val checksum: String? = null,
    val wifiOnly: Boolean = false,
) {
    companion object {
        const val STATE_QUEUED = "queued"
        const val STATE_RUNNING = "running"
        const val STATE_PAUSED = "paused"
        const val STATE_COMPLETED = "completed"
        const val STATE_FAILED = "failed"
    }
}

@Dao
interface DownloadDao {
    @Insert
    suspend fun insert(entity: DownloadEntity): Long

    @Update
    suspend fun update(entity: DownloadEntity)

    @Query("SELECT * FROM downloads ORDER BY createdAt DESC")
    fun observeAll(): Flow<List<DownloadEntity>>

    @Query("SELECT * FROM downloads WHERE id = :id")
    suspend fun byId(id: Long): DownloadEntity?

    @Query("SELECT * FROM downloads WHERE state IN ('queued', 'running', 'paused')")
    suspend fun unfinished(): List<DownloadEntity>

    @Query("SELECT * FROM downloads WHERE state = :state")
    suspend fun byState(state: String): List<DownloadEntity>

    @Query("UPDATE downloads SET state = :state WHERE id = :id")
    suspend fun setState(id: Long, state: String)

    @Query("UPDATE downloads SET downloaded = :downloaded, total = :total WHERE id = :id")
    suspend fun setProgress(id: Long, downloaded: Long, total: Long)

    @Query("UPDATE downloads SET state = :state, errorMsg = :error WHERE id = :id")
    suspend fun setError(id: Long, state: String, error: String?)

    @Query("UPDATE downloads SET state = :state, completedAt = :completedAt WHERE id = :id")
    suspend fun setCompleted(id: Long, state: String, completedAt: Long)

    @Delete
    suspend fun delete(entity: DownloadEntity)
}

@Database(entities = [DownloadEntity::class], version = 1, exportSchema = false)
abstract class AppDatabase : RoomDatabase() {
    abstract fun downloadDao(): DownloadDao

    companion object {
        @Volatile
        private var instance: AppDatabase? = null

        fun get(context: Context): AppDatabase =
            instance ?: synchronized(this) {
                instance ?: Room.databaseBuilder(
                    context.applicationContext,
                    AppDatabase::class.java,
                    "velocitydm.db",
                ).fallbackToDestructiveMigration().build().also { instance = it }
            }
    }
}
