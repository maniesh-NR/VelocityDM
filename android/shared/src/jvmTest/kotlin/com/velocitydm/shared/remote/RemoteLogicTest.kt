package com.velocitydm.shared.remote

import kotlin.test.Test
import kotlin.test.assertEquals
import kotlin.test.assertNull
import kotlin.test.assertTrue

class SseFrameParserTest {

    @Test
    fun daemonShapeSingleLineFrames() {
        val p = SseFrameParser()
        assertEquals(
            emptyList(),
            p.feed("""data: {"type":"task-added","id":1}"""),
            "no dispatch before the blank line",
        )
        assertEquals(
            listOf("""{"type":"task-added","id":1}"""),
            p.feed(""),
        )
        assertEquals(listOf("""{"type":"tasks","tasks":[]}"""), p.feed("""data: {"type":"tasks","tasks":[]}""").plus(p.feed("")))
    }

    @Test
    fun straddlingReadsAreTheReadersJobLinesAreComplete() {
        // The parser contract: it receives COMPLETE lines (BufferedReader
        // in RemoteRpc.readEvents reassembles straddling socket reads).
        // What matters here: consecutive frames never bleed into each
        // other and a data line followed by unrelated fields stays put.
        val p = SseFrameParser()
        val out = buildList {
            addAll(p.feed("""data: {"type":"task-added","id":1}"""))
            addAll(p.feed(""))
            addAll(p.feed("event: ignored"))
            addAll(p.feed("""data: {"type":"task-completed","id":7,"path":"/a/b"}"""))
            addAll(p.feed(""))
        }
        assertEquals(
            listOf(
                """{"type":"task-added","id":1}""",
                """{"type":"task-completed","id":7,"path":"/a/b"}""",
            ),
            out,
        )
    }

    @Test
    fun crlfAndLoneCrTolerated() {
        val p = SseFrameParser()
        // a stray \r (lone-CR line endings) is stripped from data lines
        // (raw strings do not process escapes — build it explicitly)
        assertEquals(emptyList(), p.feed("data: {\"a\":1}" + '\r'))
        assertEquals(listOf("""{"a":1}"""), p.feed(""))
        // a lone \r line acts as a blank dispatch line
        val p2 = SseFrameParser()
        assertEquals(emptyList(), p2.feed("""data: {"b":2}"""))
        assertEquals(listOf("""{"b":2}"""), p2.feed("\r"))
    }

    @Test
    fun commentsAndUnknownFieldsIgnored() {
        val p = SseFrameParser()
        assertEquals(emptyList(), p.feed(": keep-alive"))
        assertEquals(emptyList(), p.feed("event: tasks"))
        assertEquals(emptyList(), p.feed("id: 42"))
        assertEquals(emptyList(), p.feed("retry: 1000"))
        assertEquals(listOf("""{"ok":true}"""), p.feed("""data: {"ok":true}""").plus(p.feed("")))
    }

    @Test
    fun multiDataLinesJoinWithNewline() {
        val p = SseFrameParser()
        assertEquals(emptyList(), p.feed("data: first"))
        assertEquals(emptyList(), p.feed("data: second"))
        assertEquals(listOf("first\nsecond"), p.feed(""))
    }

    @Test
    fun oversizeFrameDroppedNotCrash() {
        val p = SseFrameParser(maxBytes = 64)
        assertEquals(emptyList(), p.feed("data: " + "x".repeat(200)))
        assertEquals(emptyList(), p.feed(""))
        assertEquals(1, p.droppedFrames())
        // parser still works after the drop
        assertEquals(listOf("""{"next":1}"""), p.feed("""data: {"next":1}""").plus(p.feed("")))
        assertEquals(1, p.droppedFrames())
    }

    @Test
    fun blankLinesWithoutDataDispatchNothing() {
        val p = SseFrameParser()
        assertEquals(emptyList(), p.feed(""))
        assertEquals(emptyList(), p.feed(""))
    }
}

class RemoteMapperTest {

    private fun snap(json: String) =
        RemoteMapper.parseList(json)!!

    @Test
    fun snapshotMapsEveryCardField() {
        val list = snap(
            """
            [{"id":5,"kind":"http","name":"big.iso","url":"https://x/y.iso","dir":"/home/me",
              "outPath":"/home/me/big.iso","state":"downloading","size":1000,"done":400,
              "speed":120,"etaSecs":5,"connections":8,"peers":0,"seeds":0,"priority":3,
              "startAt":null,"error":null,"notice":"proxy warning","checksumOk":null,
              "userPaused":false,"createdAt":123}]
            """.trimIndent()
        )
        assertEquals(1, list.size)
        val t = list[0]
        assertEquals(5L, t.id)
        assertEquals("big.iso", t.filename)
        assertEquals("https://x/y.iso", t.url)
        assertEquals("/home/me", t.dirPath)
        assertEquals(1000L, t.size)
        assertEquals(400L, t.done)
        assertEquals(120L, t.speed)
        assertEquals("downloading", t.state)
        assertNull(t.error)
        assertEquals("proxy warning", t.notice)
        assertEquals(false, t.userPaused)
    }

    @Test
    fun remoteOnlyStatesPassThrough() {
        val list = snap(
            """[{"id":1,"name":"a","url":"u","dir":"d","state":"verifying","size":9,"done":9},
                {"id":2,"name":"b","url":"u","dir":"d","state":"seeding","size":7,"done":7,"userPaused":false}]"""
        )
        assertEquals(listOf("verifying", "seeding"), list.map { it.state })
        assertNull(list[0].userPaused)
        assertEquals(false, list[1].userPaused)
    }

    @Test
    fun missingOptionalFieldsTolerated() {
        val list = snap("""[{"id":3,"name":"c","url":"u","dir":"d","state":"failed","error":"HTTP 403"}]""")
        assertNull(list[0].notice)
        assertNull(list[0].userPaused)
        assertEquals("HTTP 403", list[0].error)
    }

    @Test
    fun garbageYieldsNullNotCrash() {
        assertNull(RemoteMapper.parseList("not json"))
        assertNull(RemoteMapper.parseList("""{"object":"not array"}"""))
        // elements without an id are skipped, not fatal: an empty list
        assertEquals(emptyList(), RemoteMapper.parseList("""[{"noid":true}]"""))
    }

    @Test
    fun eventParsingRoutesTypeAndTasks() {
        val (type, tasks) = RemoteMapper.parseEvent(
            """{"type":"tasks","tasks":[{"id":9,"name":"z","url":"u","dir":"d","state":"queued"}]}"""
        )!!
        assertEquals("tasks", type)
        assertEquals(1, tasks.size)
        assertEquals(9L, tasks[0].id)
        val (t2, none) = RemoteMapper.parseEvent("""{"type":"task-removed","id":4}""")!!
        assertEquals("task-removed", t2)
        assertTrue(none.isEmpty())
        assertNull(RemoteMapper.parseEvent("garbage"))
    }

    @Test
    fun addParamsNeverCarryOutDir() {
        val opts = com.velocitydm.shared.engine.DownloadOptions(
            filename = "over.bin",
            rateLimitBps = 12345,
        )
        val p = RemoteMapper.addParams("https://x/y", opts)
        assertEquals("""{"url":"https://x/y","options":{"filename":"over.bin","rateLimitBps":12345}}""", p.toString())
        // bare add: no options object at all
        val bare = RemoteMapper.addParams("https://x/z", com.velocitydm.shared.engine.DownloadOptions())
        assertEquals("""{"url":"https://x/z"}""", bare.toString())
        assertTrue(RemoteMapper.isMagnet("magnet:?xt=urn:btih:abc"))
        assertTrue(RemoteMapper.isMagnet("MAGNET:?xt=1"))
        assertTrue(!RemoteMapper.isMagnet("https://x"))
    }
}

class RemoteConfigTest {

    private val tok = "ab".repeat(32)

    @Test
    fun validConfigBuildsHttpsBase() {
        val c = RemoteConfig(host = "192.168.1.5", port = 36990, token = tok)
        assertTrue(c.isValid())
        assertEquals("https://192.168.1.5:36990", c.baseUrl)
    }

    @Test
    fun httpUrlsAndSchemesRefused() {
        assertTrue(!RemoteConfig(host = "http://192.168.1.5", port = 36990, token = tok).isValid())
        assertTrue(!RemoteConfig(host = "https://x", port = 1, token = tok).isValid())
    }

    @Test
    fun tokenShapeEnforced() {
        assertTrue(!RemoteConfig(host = "x", port = 1, token = "short").isValid())
        assertTrue(!RemoteConfig(host = "x", port = 1, token = "Z".repeat(64)).isValid())
        assertTrue(RemoteConfig(host = "x", port = 1, token = tok).isValid())
    }

    @Test
    fun portRangeEnforced() {
        assertTrue(!RemoteConfig(host = "x", port = 0, token = tok).isValid())
        assertTrue(!RemoteConfig(host = "x", port = 70000, token = tok).isValid())
    }
}
