# Java Best Practices Skill

A Conductor skill providing guidance for writing type-safe, concurrent, modern Java 11 through Java 25 code.

## Overview

This skill activates when working with Java files and provides guidance on:

- **Type Safety**: Optional usage, @Nullable/@NonNull annotations, defensive coding patterns, Java 11+ string and Optional APIs
- **Concurrency**: CompletableFuture, Java 11 HTTP Client, virtual threads (Java 21), scoped values (Java 25), structured concurrency preview, ExecutorService, thread safety
- **Modern Features**: `var`, switch expressions, text blocks, records, sealed classes, pattern matching, compact source files, flexible constructor bodies

## Target Versions

- **Java 11 LTS**: Standard HTTP Client, single-file source launch, `String` and `Optional` conveniences, removed Java EE/CORBA modules
- **Java 12-13**: Preview-only switch expressions and text blocks; avoid using these preview forms in maintained code
- **Java 14**: Final switch expressions, helpful NullPointerException diagnostics
- **Java 15**: Final text blocks; useful for SQL, JSON, XML, and expected output
- **Java 16**: Final records and pattern matching for `instanceof`
- **Java 17 LTS**: Final sealed classes, strong JDK encapsulation by default
- **Java 18**: UTF-8 default charset; make IO charset assumptions explicit
- **Java 19-20**: Virtual thread previews; wait for Java 21 final APIs in production guidance
- **Java 21 LTS**: Final virtual threads, sequenced collections, record patterns, pattern matching in `switch`
- **Java 22**: Final Foreign Function and Memory API, unnamed variables and patterns, multi-file source launch
- **Java 23**: Markdown Javadoc; string templates withdrawn
- **Java 24**: Native access and Unsafe memory-access warnings
- **Java 25 LTS**: Scoped values, compact source files and instance main methods, flexible constructor bodies, Key Derivation Function API
- **Java 25 Preview/Incubator**: Structured concurrency, primitive patterns in `instanceof` and `switch`, module import declarations, stable values, Vector API, PEM encodings

## Activation

The skill automatically activates when:

- Working with `.java` files
- Working with `pom.xml` or `build.gradle` files
- Task description contains keywords: `java`, `optional`, `completablefuture`, `httpclient`, `text block`, `record`, `sealed`, `virtual thread`, `scoped value`
- Project tech stack includes Java

## Patterns Provided

| Pattern | Description |
|---------|-------------|
| [type-safety](patterns/type-safety.md) | Optional best practices, null safety annotations, defensive coding, Java 11+ API use |
| [concurrency](patterns/concurrency.md) | CompletableFuture, HTTP Client, virtual threads, scoped values, thread-safe patterns |
| [modern-features](patterns/modern-features.md) | `var`, switch expressions, text blocks, records, sealed classes, pattern matching, Java 25 updates |

## Usage Examples

### Type-Safe Optional Handling

```java
// Good - explicit Optional handling
public Optional<User> findUserById(String id) {
    return Optional.ofNullable(userRepository.findById(id));
}

// Usage with map/flatMap
String userName = findUserById("123")
    .map(User::getName)
    .orElse("Unknown");
```

### Modern Records

```java
// Immutable data carrier with automatic equals, hashCode, toString
public record User(String id, String name, String email) {
    // Compact constructor for validation
    public User {
        Objects.requireNonNull(id, "id cannot be null");
        Objects.requireNonNull(name, "name cannot be null");
    }
}
```

### Virtual Threads (Java 21)

```java
// Lightweight threads for IO-bound operations
try (var executor = Executors.newVirtualThreadPerTaskExecutor()) {
    List<Future<String>> futures = urls.stream()
        .map(url -> executor.submit(() -> fetchUrl(url)))
        .toList();

    for (Future<String> future : futures) {
        System.out.println(future.get());
    }
}
```

### Scoped Values (Java 25)

```java
private static final ScopedValue<RequestContext> REQUEST_CONTEXT =
    ScopedValue.newInstance();

ScopedValue.where(REQUEST_CONTEXT, context)
    .run(() -> service.handle(request));
```

## Related Skills

- **testing-strategies**: For Java testing patterns with JUnit 5
- **api-design**: For REST API development with Spring Boot or Jakarta EE

## Changelog

### 1.2.0

- Added Java 11 HTTP Client, text blocks, UTF-8 default charset, sequenced collections, FFM API, unnamed variables, and Markdown Javadoc guidance
- Expanded Java 11 through Java 25 best-practice guidance

### 1.1.0

- Updated target guidance for Java 25 LTS
- Added scoped values, structured concurrency preview, compact source files, flexible constructor bodies, module import declarations preview, and primitive pattern preview guidance

### 1.0.0

- Initial release
- Type safety patterns with Optional and null annotations
- Concurrency patterns with CompletableFuture and virtual threads
- Modern Java features (records, sealed classes, pattern matching)
