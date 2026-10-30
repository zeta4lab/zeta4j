// Movie Graph Dataset
// Movies example: actors, directors and the films they worked on
//
// Usage:
//   z4j shell -f examples/movies/data.cypher

// Create Movies
CREATE (TheMatrix:Movie {title: 'The Matrix', released: 1999, tagline: 'Welcome to the Real World'});
CREATE (TheMatrixReloaded:Movie {title: 'The Matrix Reloaded', released: 2003, tagline: 'Free your mind'});
CREATE (TheMatrixRevolutions:Movie {title: 'The Matrix Revolutions', released: 2003, tagline: 'Everything that has a beginning has an end'});
CREATE (TopGun:Movie {title: 'Top Gun', released: 1986, tagline: 'I feel the need, the need for speed.'});
CREATE (JerryMaguire:Movie {title: 'Jerry Maguire', released: 2000, tagline: 'The rest of his life begins now.'});
CREATE (AFewGoodMen:Movie {title: 'A Few Good Men', released: 1992, tagline: "In the heart of the nation's capital, in a courthouse of the U.S. government, one man will stop at nothing to keep his honor, and one will stop at nothing to find the truth."});
CREATE (TheGodfather:Movie {title: 'The Godfather', released: 1972, tagline: "An offer you can't refuse."});
CREATE (Inception:Movie {title: 'Inception', released: 2010, tagline: 'Your mind is the scene of the crime'});
CREATE (Interstellar:Movie {title: 'Interstellar', released: 2014, tagline: 'Mankind was born on Earth. It was never meant to die here.'});
CREATE (TheDarkKnight:Movie {title: 'The Dark Knight', released: 2008, tagline: 'Why So Serious?'});
CREATE (Forrest:Movie {title: 'Forrest Gump', released: 1994, tagline: 'Life is like a box of chocolates.'});
CREATE (CastAway:Movie {title: 'Cast Away', released: 2000, tagline: 'At the edge of the world, his journey begins.'});
CREATE (Apollo13:Movie {title: 'Apollo 13', released: 1995, tagline: 'Houston, we have a problem.'});

// Create Persons
CREATE (Keanu:Person {name: 'Keanu Reeves', born: 1964});
CREATE (Carrie:Person {name: 'Carrie-Anne Moss', born: 1967});
CREATE (Laurence:Person {name: 'Laurence Fishburne', born: 1961});
CREATE (Hugo:Person {name: 'Hugo Weaving', born: 1960});
CREATE (LillyW:Person {name: 'Lilly Wachowski', born: 1967});
CREATE (LanaW:Person {name: 'Lana Wachowski', born: 1965});
CREATE (TomC:Person {name: 'Tom Cruise', born: 1962});
CREATE (JackN:Person {name: 'Jack Nicholson', born: 1937});
CREATE (DemiM:Person {name: 'Demi Moore', born: 1962});
CREATE (KevinB:Person {name: 'Kevin Bacon', born: 1958});
CREATE (AlP:Person {name: 'Al Pacino', born: 1940});
CREATE (MarlonB:Person {name: 'Marlon Brando', born: 1924});
CREATE (FrancisCoppola:Person {name: 'Francis Ford Coppola', born: 1939});
CREATE (Leonardo:Person {name: 'Leonardo DiCaprio', born: 1974});
CREATE (ChristopherN:Person {name: 'Christopher Nolan', born: 1970});
CREATE (MatthewM:Person {name: 'Matthew McConaughey', born: 1969});
CREATE (AnneH:Person {name: 'Anne Hathaway', born: 1982});
CREATE (ChristianB:Person {name: 'Christian Bale', born: 1974});
CREATE (HeathL:Person {name: 'Heath Ledger', born: 1979});
CREATE (TomH:Person {name: 'Tom Hanks', born: 1956});
CREATE (RobinW:Person {name: 'Robin Wright', born: 1966});
CREATE (GaryS:Person {name: 'Gary Sinise', born: 1955});
CREATE (RobertZ:Person {name: 'Robert Zemeckis', born: 1952});
CREATE (EdH:Person {name: 'Ed Harris', born: 1950});
CREATE (BillP:Person {name: 'Bill Paxton', born: 1955});
CREATE (RonH:Person {name: 'Ron Howard', born: 1954});

// Create Relationships - ACTED_IN
MATCH (keanu:Person {name: 'Keanu Reeves'}), (matrix:Movie {title: 'The Matrix'})
CREATE (keanu)-[:ACTED_IN {roles: ['Neo']}]->(matrix);

MATCH (keanu:Person {name: 'Keanu Reeves'}), (matrix:Movie {title: 'The Matrix Reloaded'})
CREATE (keanu)-[:ACTED_IN {roles: ['Neo']}]->(matrix);

MATCH (keanu:Person {name: 'Keanu Reeves'}), (matrix:Movie {title: 'The Matrix Revolutions'})
CREATE (keanu)-[:ACTED_IN {roles: ['Neo']}]->(matrix);

MATCH (carrie:Person {name: 'Carrie-Anne Moss'}), (matrix:Movie {title: 'The Matrix'})
CREATE (carrie)-[:ACTED_IN {roles: ['Trinity']}]->(matrix);

MATCH (carrie:Person {name: 'Carrie-Anne Moss'}), (matrix:Movie {title: 'The Matrix Reloaded'})
CREATE (carrie)-[:ACTED_IN {roles: ['Trinity']}]->(matrix);

MATCH (carrie:Person {name: 'Carrie-Anne Moss'}), (matrix:Movie {title: 'The Matrix Revolutions'})
CREATE (carrie)-[:ACTED_IN {roles: ['Trinity']}]->(matrix);

MATCH (laurence:Person {name: 'Laurence Fishburne'}), (matrix:Movie {title: 'The Matrix'})
CREATE (laurence)-[:ACTED_IN {roles: ['Morpheus']}]->(matrix);

MATCH (hugo:Person {name: 'Hugo Weaving'}), (matrix:Movie {title: 'The Matrix'})
CREATE (hugo)-[:ACTED_IN {roles: ['Agent Smith']}]->(matrix);

MATCH (tom:Person {name: 'Tom Cruise'}), (movie:Movie {title: 'Top Gun'})
CREATE (tom)-[:ACTED_IN {roles: ['Maverick']}]->(movie);

MATCH (tom:Person {name: 'Tom Cruise'}), (movie:Movie {title: 'Jerry Maguire'})
CREATE (tom)-[:ACTED_IN {roles: ['Jerry Maguire']}]->(movie);

MATCH (tom:Person {name: 'Tom Cruise'}), (movie:Movie {title: 'A Few Good Men'})
CREATE (tom)-[:ACTED_IN {roles: ['Lt. Daniel Kaffee']}]->(movie);

MATCH (jack:Person {name: 'Jack Nicholson'}), (movie:Movie {title: 'A Few Good Men'})
CREATE (jack)-[:ACTED_IN {roles: ['Col. Nathan R. Jessup']}]->(movie);

MATCH (demi:Person {name: 'Demi Moore'}), (movie:Movie {title: 'A Few Good Men'})
CREATE (demi)-[:ACTED_IN {roles: ['Lt. Cdr. JoAnne Galloway']}]->(movie);

MATCH (kevin:Person {name: 'Kevin Bacon'}), (movie:Movie {title: 'A Few Good Men'})
CREATE (kevin)-[:ACTED_IN {roles: ['Capt. Jack Ross']}]->(movie);

MATCH (al:Person {name: 'Al Pacino'}), (movie:Movie {title: 'The Godfather'})
CREATE (al)-[:ACTED_IN {roles: ['Michael Corleone']}]->(movie);

MATCH (marlon:Person {name: 'Marlon Brando'}), (movie:Movie {title: 'The Godfather'})
CREATE (marlon)-[:ACTED_IN {roles: ['Vito Corleone']}]->(movie);

MATCH (leo:Person {name: 'Leonardo DiCaprio'}), (movie:Movie {title: 'Inception'})
CREATE (leo)-[:ACTED_IN {roles: ['Cobb']}]->(movie);

MATCH (matthew:Person {name: 'Matthew McConaughey'}), (movie:Movie {title: 'Interstellar'})
CREATE (matthew)-[:ACTED_IN {roles: ['Cooper']}]->(movie);

MATCH (anne:Person {name: 'Anne Hathaway'}), (movie:Movie {title: 'Interstellar'})
CREATE (anne)-[:ACTED_IN {roles: ['Brand']}]->(movie);

MATCH (anne:Person {name: 'Anne Hathaway'}), (movie:Movie {title: 'The Dark Knight'})
CREATE (anne)-[:ACTED_IN {roles: ['Selina Kyle']}]->(movie);

MATCH (christian:Person {name: 'Christian Bale'}), (movie:Movie {title: 'The Dark Knight'})
CREATE (christian)-[:ACTED_IN {roles: ['Bruce Wayne']}]->(movie);

MATCH (heath:Person {name: 'Heath Ledger'}), (movie:Movie {title: 'The Dark Knight'})
CREATE (heath)-[:ACTED_IN {roles: ['Joker']}]->(movie);

MATCH (tom:Person {name: 'Tom Hanks'}), (movie:Movie {title: 'Forrest Gump'})
CREATE (tom)-[:ACTED_IN {roles: ['Forrest Gump']}]->(movie);

MATCH (robin:Person {name: 'Robin Wright'}), (movie:Movie {title: 'Forrest Gump'})
CREATE (robin)-[:ACTED_IN {roles: ['Jenny Curran']}]->(movie);

MATCH (gary:Person {name: 'Gary Sinise'}), (movie:Movie {title: 'Forrest Gump'})
CREATE (gary)-[:ACTED_IN {roles: ['Lt. Dan Taylor']}]->(movie);

MATCH (tom:Person {name: 'Tom Hanks'}), (movie:Movie {title: 'Cast Away'})
CREATE (tom)-[:ACTED_IN {roles: ['Chuck Noland']}]->(movie);

MATCH (tom:Person {name: 'Tom Hanks'}), (movie:Movie {title: 'Apollo 13'})
CREATE (tom)-[:ACTED_IN {roles: ['Jim Lovell']}]->(movie);

MATCH (ed:Person {name: 'Ed Harris'}), (movie:Movie {title: 'Apollo 13'})
CREATE (ed)-[:ACTED_IN {roles: ['Gene Kranz']}]->(movie);

MATCH (bill:Person {name: 'Bill Paxton'}), (movie:Movie {title: 'Apollo 13'})
CREATE (bill)-[:ACTED_IN {roles: ['Fred Haise']}]->(movie);

MATCH (gary:Person {name: 'Gary Sinise'}), (movie:Movie {title: 'Apollo 13'})
CREATE (gary)-[:ACTED_IN {roles: ['Ken Mattingly']}]->(movie);

// Create Relationships - DIRECTED
MATCH (lilly:Person {name: 'Lilly Wachowski'}), (matrix:Movie {title: 'The Matrix'})
CREATE (lilly)-[:DIRECTED]->(matrix);

MATCH (lana:Person {name: 'Lana Wachowski'}), (matrix:Movie {title: 'The Matrix'})
CREATE (lana)-[:DIRECTED]->(matrix);

MATCH (francis:Person {name: 'Francis Ford Coppola'}), (movie:Movie {title: 'The Godfather'})
CREATE (francis)-[:DIRECTED]->(movie);

MATCH (chris:Person {name: 'Christopher Nolan'}), (movie:Movie {title: 'Inception'})
CREATE (chris)-[:DIRECTED]->(movie);

MATCH (chris:Person {name: 'Christopher Nolan'}), (movie:Movie {title: 'Interstellar'})
CREATE (chris)-[:DIRECTED]->(movie);

MATCH (chris:Person {name: 'Christopher Nolan'}), (movie:Movie {title: 'The Dark Knight'})
CREATE (chris)-[:DIRECTED]->(movie);

MATCH (robert:Person {name: 'Robert Zemeckis'}), (movie:Movie {title: 'Forrest Gump'})
CREATE (robert)-[:DIRECTED]->(movie);

MATCH (robert:Person {name: 'Robert Zemeckis'}), (movie:Movie {title: 'Cast Away'})
CREATE (robert)-[:DIRECTED]->(movie);

MATCH (ron:Person {name: 'Ron Howard'}), (movie:Movie {title: 'Apollo 13'})
CREATE (ron)-[:DIRECTED]->(movie);

// Create Relationships - PRODUCED
MATCH (chris:Person {name: 'Christopher Nolan'}), (movie:Movie {title: 'Inception'})
CREATE (chris)-[:PRODUCED]->(movie);

MATCH (chris:Person {name: 'Christopher Nolan'}), (movie:Movie {title: 'Interstellar'})
CREATE (chris)-[:PRODUCED]->(movie);
