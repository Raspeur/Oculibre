package org.oculibre

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test
import java.io.File

class ProblemTest {
    @Test
    fun codesAreUniqueAndInTheirFamily() {
        val codes = Problem.values().map { it.code }
        assertEquals(codes.size, codes.toSet().size)
        assertEquals(Problem.NOT_RECOGNISED, Problem.ofCode(203))
        // Same numbers as the computer's catalogue (design/mockups/Problems) for the same problem.
        assertEquals(101, Problem.SEARCHING.code)
        assertEquals(301, Problem.CAMERA_BUSY.code)
        assertEquals(305, Problem.CAMERA_PERMISSION.code)
        assertTrue(Problem.CAMERA_PERMISSION.blocking)
    }

    @Test
    fun byeReasons() {
        assertNull(Problem.fromBye("user_stop"))
        assertNull(Problem.fromBye("shutdown"))
        assertEquals(Problem.NOT_RECOGNISED, Problem.fromBye("unpaired"))
        assertEquals(Problem.UPDATE_NEEDED, Problem.fromBye("version"))
        assertEquals(Problem.COMPUTER_BUSY, Problem.fromBye("busy"))
        assertEquals(Problem.UNEXPECTED, Problem.fromBye("something_new"))
    }

    @Test
    fun cameraErrors() {
        // CameraDevice.StateCallback.ERROR_CAMERA_IN_USE = 1, ERROR_CAMERA_DEVICE = 4
        assertEquals(Problem.CAMERA_BUSY, Problem.fromCameraError(1))
        assertEquals(Problem.CAMERA_ERROR, Problem.fromCameraError(4))
        // CameraAccessException.CAMERA_IN_USE = 4, CAMERA_DISABLED = 1
        assertEquals(Problem.CAMERA_BUSY, Problem.fromCameraAccess(4))
        assertEquals(Problem.CAMERA_ERROR, Problem.fromCameraAccess(1))
    }

    /** Every problem says something in every language of the app. */
    @Test
    fun everyProblemIsTranslated() {
        for (dir in listOf("values", "values-fr")) {
            val xml = File("src/main/res/$dir/strings.xml").readText()
            for (p in Problem.values()) {
                for (part in listOf("title", "text")) {
                    assertTrue("$dir: problem_${p.code}_$part", xml.contains("name=\"problem_${p.code}_$part\""))
                }
            }
        }
    }

    /** Both languages have the same strings (none forgotten in French). */
    @Test
    fun frenchHasEveryString() {
        val names = Regex("<string name=\"([a-z0-9_]+)\"")
        fun keys(dir: String) = names.findAll(File("src/main/res/$dir/strings.xml").readText()).map { it.groupValues[1] }.toSet()
        val missing = keys("values") - keys("values-fr") - setOf("app_name") // a name: never translated
        assertTrue("missing in French: $missing", missing.isEmpty())
    }
}
