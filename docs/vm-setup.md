# VM setup

## VM

- Claude and git
- JDK 8, 11, 17, 21, and 25
- Default `JAVA_HOME` points to JDK 25
- All installed JDKs are addressable by stable paths for Maven and Gradle toolchains
- Apache Maven 3.9.16
- Gradle 9.7.0
- Gradle wrapper support enabled; prefer project `./gradlew` when present
- Network access for dependency resolution during project build validation
