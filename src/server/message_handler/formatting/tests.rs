use indoc::indoc;
use tree_sitter_c2rust::Parser;

use crate::server::message_handler::formatting::format_helper;

fn format_and_compare(ugly_query: &str, pretty_query: &str) {
    let mut parser = Parser::new();
    parser
        .set_language(&tree_sitter_sparql::language())
        .unwrap();
    let tree = parser.parse(ugly_query, None).unwrap();
    let formatted_query = format_helper(
        &ugly_query.to_string(),
        &mut tree.root_node().walk(),
        0,
        "  ",
        "",
    );
    assert_eq!(formatted_query, pretty_query);
}
#[test]
fn prologue() {
    let ugly_query = indoc!(
        "
              PReFIX   namespace:  <uri>

            prefix namespace:  <uri>
            SELECT * {}"
    );
    let pretty_query = indoc!(
        "PREFIX namespace: <uri>
         PREFIX namespace: <uri>
         SELECT * {}"
    );
    format_and_compare(ugly_query, pretty_query);
}

#[test]
fn nesting_indentation() {
    let ugly_query = "SELECT * {{{SELECT * {?a ?a ?a}}}}";
    let pretty_query = indoc!(
        "SELECT * {
           {
             {
               SELECT * {
                 ?a ?a ?a
               }
             }
           }
         }"
    );
    format_and_compare(ugly_query, pretty_query);
}
#[test]
fn alternating_group_graph_pattern() {
    let ugly_query = indoc!(
        "SELECT * {
             ?a ?c ?b .{
             } ?a ?b ?c
             }"
    );
    let pretty_query = indoc!(
        "SELECT * {
           ?a ?c ?b .
           {}
           ?a ?b ?c
         }"
    );
    format_and_compare(ugly_query, pretty_query);
}

#[test]
fn union() {
    let ugly_query = indoc!(
        "SELECT * {
           {} UNION {}
             }"
    );
    let pretty_query = indoc!(
        "SELECT * {
           {}
           UNION
           {}
         }"
    );
    format_and_compare(ugly_query, pretty_query)
}

#[test]
fn select_clause() {
    let ugly_query = indoc!("SELECT ( <> as ?a) ?a  *{}");
    let pretty_query = indoc!("SELECT (<> AS ?a) ?a * {}");
    format_and_compare(ugly_query, pretty_query)
}

#[test]
fn optional() {
    let ugly_query = indoc!(
        "SELECT * {
             Optional
             {
             ?a ?c ?c}
             }"
    );
    let pretty_query = indoc!(
        "SELECT * {
           OPTIONAL {
             ?a ?c ?c
           }
         }"
    );
    format_and_compare(ugly_query, pretty_query)
}

#[test]
fn minus() {
    let ugly_query = indoc!(
        "SELECT * {
             {} minus {}
             }"
    );
    let pretty_query = indoc!(
        "SELECT * {
           {}
           MINUS
           {}
         }"
    );
    format_and_compare(ugly_query, pretty_query)
}

#[test]
fn graph() {
    let ugly_query = indoc!(
        "SELECT * {
             {} Graph ?a { ?a ?b  ?c}
             }"
    );
    let pretty_query = indoc!(
        "SELECT * {
           {}
           GRAPH ?a {
             ?a ?b ?c
           }
         }"
    );
    format_and_compare(ugly_query, pretty_query)
}

#[test]
fn filter() {
    let ugly_query = indoc!("SELECT * {filter   (1>0)}");
    let pretty_query = indoc!(
        "SELECT * {
           FILTER (1 > 0)
         }"
    );
    format_and_compare(ugly_query, pretty_query)
}

#[test]
fn binary_expression() {
    let ugly_query = indoc!(
        "SELECT * {
            filter (1 = 3+2-2.9*10/0 && 
                    1 > 2 ||
                    1 < 2 ||
                    1 <= 2 &&
                    1 >= 9 ||
                    1 != 3 ||
                    5 in (1,2,3) &&
                    6 not in (4,5,6+3))}"
    );
    let pretty_query = indoc!(
        "SELECT * {
           FILTER (1 = 3 + 2 - 2.9 * 10 / 0 && 1 > 2 || 1 < 2 || 1 <= 2 && 1 >= 9 || 1 != 3 || 5 IN (1, 2, 3) && 6 NOT IN (4, 5, 6 + 3))
         }"
    );
    format_and_compare(ugly_query, pretty_query)
}

#[test]
fn bind() {
    let ugly_query = indoc!("SELECT * {Bind (1 as ?var )}");
    let pretty_query = indoc!(
        "SELECT * {
           BIND ( 1 AS ?var )
         }"
    );
    format_and_compare(ugly_query, pretty_query)
}

#[test]
fn inline_data() {
    let ugly_query = indoc!("SELECT * {values ?a { 1 2 3}}");
    let pretty_query = indoc!(
        "SELECT * {
           VALUES ?a { 1 2 3 }
         }"
    );
    format_and_compare(ugly_query, pretty_query)
}

#[test]
fn values_clause() {
    let ugly_query = indoc!("SELECT * {}values ?a { 1 2 3}");
    let pretty_query = indoc!(
        "SELECT * {}
         VALUES ?a { 1 2 3 }"
    );
    format_and_compare(ugly_query, pretty_query)
}

#[test]
fn solution_modifier() {
    let ugly_query = indoc!(
        "SELECT * WHERE {}
           GROUP by ( 2 AS ?a )
          HAVING (2 > 2) (1 > 2)
            order BY ASC (?c)
         OFfSET 3 LiMIT 3"
    );
    let pretty_query = indoc!(
        "SELECT * WHERE {}
         GROUP BY ( 2 AS ?a )
         HAVING (2 > 2) (1 > 2)
         ORDER BY ASC (?c)
         OFFSET 3 LIMIT 3"
    );
    format_and_compare(ugly_query, pretty_query)
}

#[test]
fn dataset_clause() {
    let ugly_query = indoc!(
        "PREFIX foaf: <http://xmlns.com/foaf/0.1/>
         SELECT  ?name
         FROM    <http://example.org/foaf/aliceFoaf>
         WHERE   { ?x foaf:name ?name }"
    );
    let pretty_query = indoc!(
        "PREFIX foaf: <http://xmlns.com/foaf/0.1/>
         SELECT ?name FROM <http://example.org/foaf/aliceFoaf> WHERE {
           ?x foaf:name ?name
         }"
    );
    format_and_compare(ugly_query, pretty_query)
}

#[test]
fn construct() {
    let ugly_query = indoc!(
        "PREFIX foaf:    <http://xmlns.com/foaf/0.1/>
         PREFIX vcard:   <http://www.w3.org/2001/vcard-rdf/3.0#>
         CONSTRUCT   { <http://example.org/person#Alice> vcard:FN ?name }
         WHERE       { ?x foaf:name ?name }"
    );
    let pretty_query = indoc!(
        "PREFIX foaf:  <http://xmlns.com/foaf/0.1/>
         PREFIX vcard: <http://www.w3.org/2001/vcard-rdf/3.0#>
         CONSTRUCT {
           <http://example.org/person#Alice> vcard:FN ?name
         }
         WHERE {
           ?x foaf:name ?name
         }"
    );
    format_and_compare(ugly_query, pretty_query)
}

#[test]
fn describe() {
    let ugly_query = indoc!(
        "PREFIX foaf:   <http://xmlns.com/foaf/0.1/>
         DESCRIBE ?x ?y <http://example.org/>
         WHERE    {?x foaf:knows ?y}"
    );
    let pretty_query = indoc!(
        "PREFIX foaf: <http://xmlns.com/foaf/0.1/>
         DESCRIBE ?x ?y <http://example.org/> WHERE {
           ?x foaf:knows ?y
         }"
    );
    format_and_compare(ugly_query, pretty_query)
}

#[test]
fn ask() {
    let ugly_query = indoc!(
        r#"PREFIX foaf:    <http://xmlns.com/foaf/0.1/>
           ASK  { ?x foaf:name  "Alice" }
           "#
    );
    let pretty_query = indoc!(
        r#"PREFIX foaf: <http://xmlns.com/foaf/0.1/>
           ASK {
             ?x foaf:name "Alice"
           }"#
    );
    format_and_compare(ugly_query, pretty_query)
}

#[test]
fn graph_management() {
    let ugly_query = indoc!(
        "load SIlENT <a> INTO graph <c> ;
              LOAD    <b>; Clear Graph <b>          ;
          drop   graph<c>  ; ADD SILENT GRAPH <c> to DEFAULT ;MOVE default TO GRAPH <a> ;
                  create graph <d>"
    );
    let pretty_query = indoc!(
        "LOAD SILENT <a> INTO GRAPH <c> ;
         LOAD <b> ;
         CLEAR GRAPH <b> ;
         DROP GRAPH <c> ;
         ADD SILENT GRAPH <c> TO DEFAULT ;
         MOVE DEFAULT TO GRAPH <a> ;
         CREATE GRAPH <d>"
    );
    format_and_compare(ugly_query, pretty_query)
}

#[test]
fn insert_data() {
    let ugly_query = indoc!(
        "Insert   data
         {
            ?a ?b ?c.
             graph <a> {
         ?c ?b ?a }.
         ?d ?e ?f
         graph  ?d 
         {
         ?a ?d ?c
         }
         ?d ?e ?f
         }"
    );
    let pretty_query = indoc!(
        "INSERT DATA {
           ?a ?b ?c .
           GRAPH <a> {
             ?c ?b ?a
           } .
           ?d ?e ?f
           GRAPH ?d {
             ?a ?d ?c
           }
           ?d ?e ?f
         }"
    );
    format_and_compare(ugly_query, pretty_query)
}

#[test]
fn delete_data() {
    let ugly_query = indoc!(
        "delete   data
         {
            ?a ?b ?c.
             graph <a> {
         ?c ?b ?a }.
         ?d ?e ?f
         graph  ?d 
         {
         ?a ?d ?c
         }
         ?d ?e ?f
         }"
    );
    let pretty_query = indoc!(
        "DELETE DATA {
           ?a ?b ?c .
           GRAPH <a> {
             ?c ?b ?a
           } .
           ?d ?e ?f
           GRAPH ?d {
             ?a ?d ?c
           }
           ?d ?e ?f
         }"
    );
    format_and_compare(ugly_query, pretty_query)
}

#[test]
fn delete_where() {
    let ugly_query = indoc!(
        "delete   where
         {
            ?a ?b ?c.
             graph <a> {
         ?c ?b ?a }.
         ?d ?e ?f
         graph  ?d 
         {
         ?a ?d ?c
         }
         ?d ?e ?f
         }"
    );
    let pretty_query = indoc!(
        "DELETE WHERE {
           ?a ?b ?c .
           GRAPH <a> {
             ?c ?b ?a
           } .
           ?d ?e ?f
           GRAPH ?d {
             ?a ?d ?c
           }
           ?d ?e ?f
         }"
    );
    format_and_compare(ugly_query, pretty_query)
}

#[test]
fn modify() {
    let ugly_query = indoc!(
        "With <a> delete
         { 
         ?a  ?b   ?C   
          } insert { ?x ?y ?z } using <a> using named <b> where
             {
         { ?a ?b ?c  .  }
         }"
    );
    let pretty_query = indoc!(
        "WITH <a>
         DELETE {
           ?a ?b ?C
         }
         INSERT {
           ?x ?y ?z
         }
         USING <a>
         USING NAMED <b>
         WHERE {
           {
             ?a ?b ?c .
           }
         }"
    );
    format_and_compare(ugly_query, pretty_query)
}

#[test]
fn property_paths() {
    let ugly_query = indoc!(
        "SELECT *
         WHERE  { ?P foaf:givenName ?G ; foaf:surname ?S }"
    );
    let pretty_query = indoc!(
        "SELECT * WHERE {
           ?P foaf:givenName ?G ;
              foaf:surname ?S
         }"
    );
    format_and_compare(ugly_query, pretty_query)
}

#[test]
fn property_list_paths() {
    let ugly_query = indoc!(
        "SELECT * WHERE {
           ?a          <iri>/^a/(!<>?)+   |           (<iri> 
         | ^a |  a) ?b .
         }"
    );
    let pretty_query = indoc!(
        "SELECT * WHERE {
           ?a <iri>/^a/(!<>?)+ | (<iri> | ^a | a) ?b .
         }"
    );
    format_and_compare(ugly_query, pretty_query)
}

#[test]
fn prefix_indentation() {
    let ugly_query = indoc!(
        "PREFIX hello_world: <http://xmlns.com/foaf/0.1/>
         PREFIX hello_worl: <http://xmlns.com/foaf/0.1/>
         PREFIX hello_wor: <http://xmlns.com/foaf/0.1/>
         PREFIX hello_wo: <http://xmlns.com/foaf/0.1/>
         PREFIX hello_w: <http://xmlns.com/foaf/0.1/>
         PREFIX hello_: <http://xmlns.com/foaf/0.1/>
         PREFIX hello: <http://xmlns.com/foaf/0.1/>
         PREFIX hell: <http://xmlns.com/foaf/0.1/>
         PREFIX hel: <http://xmlns.com/foaf/0.1/>
         PREFIX he: <http://xmlns.com/foaf/0.1/>
         PREFIX h: <http://xmlns.com/foaf/0.1/>
         SELECT * {}"
    );
    let pretty_query = indoc!(
        "PREFIX hello_world: <http://xmlns.com/foaf/0.1/>
         PREFIX hello_worl:  <http://xmlns.com/foaf/0.1/>
         PREFIX hello_wor:   <http://xmlns.com/foaf/0.1/>
         PREFIX hello_wo:    <http://xmlns.com/foaf/0.1/>
         PREFIX hello_w:     <http://xmlns.com/foaf/0.1/>
         PREFIX hello_:      <http://xmlns.com/foaf/0.1/>
         PREFIX hello:       <http://xmlns.com/foaf/0.1/>
         PREFIX hell:        <http://xmlns.com/foaf/0.1/>
         PREFIX hel:         <http://xmlns.com/foaf/0.1/>
         PREFIX he:          <http://xmlns.com/foaf/0.1/>
         PREFIX h:           <http://xmlns.com/foaf/0.1/>
         SELECT * {}"
    );
    format_and_compare(ugly_query, pretty_query)
}

#[test]
fn prefix_indentation2() {
    let ugly_query = indoc!(
        "PREFIX aa: <>
         PREFIX a:   <>
         PREFIX a:   <>
         SELECT * {}"
    );
    let pretty_query = indoc!(
        "PREFIX aa: <>
         PREFIX a:  <>
         PREFIX a:  <>
         SELECT * {}"
    );
    format_and_compare(ugly_query, pretty_query)
}
