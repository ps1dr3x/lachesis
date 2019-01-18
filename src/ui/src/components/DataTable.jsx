import React, { useState, useEffect } from 'react'
import {
  Segment,
  Dimmer,
  Loader,
  Label,
  Table
} from 'semantic-ui-react'
import uuid from 'uuid/v4'
import 'style/data-table.scss'

/* global fetch */

function DataTable () {
  const [loading, setLoading] = useState(true)
  const [data, setData] = useState(null)

  async function getData () {
    let services = null
    try {
      services = await fetch('api/services')
        .then((res) => res.json())
    } catch (ex) {}

    setLoading(false)

    if (services === null) {
      setData(null)
    } else {
      setData({
        headers: Object.keys(services[0]),
        rows: services.map((row) => Object.values(row))
      })
    }
  }

  useEffect(() => {
    getData()
  }, {})

  if (loading) {
    return (
      <div className='data-table'>
        <Segment>
          <Dimmer active inverted>
            <Loader size='massive' />
          </Dimmer>
        </Segment>
      </div>
    )
  }

  if (data === null) {
    return <p>Fetch error</p>
  }

  return (
    <div className='data-table'>
      <Table celled>
        <Table.Header>
          <Table.Row>
            {
              data.headers.map((el) => {
                return <Table.HeaderCell key={uuid()}>{el}</Table.HeaderCell>
              })
            }
          </Table.Row>
        </Table.Header>
        <Table.Body>
          {
            data.rows.map((fields) => {
              let cells = []
              for (let field of fields) {
                cells.push(<Table.Cell key={uuid()}><Label>{field}</Label></Table.Cell>)
              }
              return <Table.Row key={fields[0]}>{cells}</Table.Row>
            })
          }
        </Table.Body>
      </Table>
    </div>
  )
}

export default DataTable
