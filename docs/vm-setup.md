# VM setup

## VM

- Claude and git
- JDK 8, 11, 17, 21, and 25
- Default `JAVA_HOME` points to JDK 25
- All installed JDKs are addressable under `/opt/jdks`:
  - `/opt/jdks/jdk8`
  - `/opt/jdks/jdk11`
  - `/opt/jdks/jdk17`
  - `/opt/jdks/jdk21`
  - `/opt/jdks/jdk25`
- Java compatibility analyzer uses `GLUON_JDK_ROOT` when set, otherwise `/opt/jdks`, as the default JDK root for optional `jdeps` and `jdeprscan` enrichment.
- Apache Maven 3.9.16
- Gradle 9.7.0
- Gradle wrapper support enabled; prefer project `./gradlew` when present
- Network access for dependency resolution during project build validation
- Eclipse JDTLS.

