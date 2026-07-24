package app.platform;

// cm: def SystemStreams
public final class SystemStreams {
	// cm: def SystemStreams.writeBoth
	public static void writeBoth(String message) {
		// cm: ref SystemStreams.writeBoth.calls.out.println
		System.out.println(message);
		// cm: ref SystemStreams.writeBoth.calls.err.println
		System.err.println(message);
	}
}
